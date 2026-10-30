# SPDX-License-Identifier: GPL-3.0-or-later
"""mental ray shader approximations (unit sky1; our own code).

The Softimage renderer this add-on ports never ran the mental ray shaders a scene can carry: the original handed them to
mental ray (liboz.so), which is not part of the port.  For the scenes that carry them, this module gives a stand-in that
follows each shader's documented intent with what the Fogle3D engine already does; it is NOT the bit-accurate path and
is used only when the imported scene holds such a shader (the import dump's `shaders` table, si-import-dump).  See
docs/design/softimage-import.md, "mental ray shaders: approximations".

  Env_Atmosphere (material shader, the sky dome): a single-scattering Rayleigh + Mie sky over a spherical planet, sun
      disc and halo, a moon lit by the sun, stars, a fractal cloud layer, gain; evaluated per camera ray and put behind
      the engine's picture (the dome itself is not rendered), and as a latitude-longitude reflection map (method 7: ray
      traced first, the map where nothing is hit) on the reflective materials of the scene that have none.
  Water (2D texture shader): a sum of sine waves (deep-water dispersion), trough to crest colour, written into the
      texture's picture each frame; the engine's own texture blend and bump do the rest.

Everything here except the `import_*` / `render_*` hooks is plain numpy (no bpy), so the pictures can be made and
checked outside Blender (re/tools/sky1)."""
import json
import math
import os

import numpy as np

ATMOSPHERE = "Env_Atmosphere.mat"
WATER = "Water.tex"
SKY_PROP = "si3d_mr_sky"            # scene: JSON {"shader": the dump's shader record, "rate": frames per second}
SKY_OFF_PROP = "si3d_mr_sky_off"    # scene: the same, while the sky is switched off (switch_sky)
WATER_PROP = "si3d_mr_water"        # image: JSON {"shader": ..., "rate": ...}
SKYMAP_PROP = "si3d_mr_skymap"      # image: the sky as a reflection map (marker)
DOME_PROP = "si3d_mr_dome"          # object: the dome the sky shader was on (not rendered)
ENV_SIZE = (1024, 512)
WATER_SIZE = 512


def library(shader):
    """The library file name a shader element instances (e.g. 'Env_Atmosphere.mat')."""
    return os.path.basename(str(shader.get("library") or ""))


# ------------------------------------------------------------------------------------------------- parameters
def _interp(keys, vals, t):
    if t <= keys[0]:
        return float(vals[0])
    for k in range(1, len(keys)):
        if t <= keys[k]:
            a = (t - keys[k - 1]) / max(keys[k] - keys[k - 1], 1e-9)
            return float(vals[k - 1]) + (float(vals[k]) - float(vals[k - 1])) * a
    return float(vals[-1])


def params_at(shader, t):
    """The shader's parameters at time `t` (seconds): a keyed parameter (one value per key time) linearly between its
    keys, held before the first and after the last; colours gathered from name.r/.g/.b(/.a) into lists."""
    keys = [float(k) for k in (shader.get("keys") or [])]
    out = {}
    for name, vals in (shader.get("params") or {}).items():
        nums = all(isinstance(v, (int, float)) for v in vals)
        if len(keys) > 1 and len(vals) == len(keys) and nums:
            out[name] = _interp(keys, vals, t)
        elif len(vals) == 1:
            out[name] = vals[0]
        elif not vals:
            out[name] = ""
        else:
            out[name] = list(vals)
    for name in list(out):
        if name.endswith(".r"):
            base = name[:-2]
            out[base] = [float(out.get(base + s, 0.0)) for s in (".r", ".g", ".b")]
            if base + ".a" in out:
                out[base].append(float(out[base + ".a"]))
    return out


def _f(p, k, d):
    v = p.get(k, d)
    try:
        return float(v)
    except (TypeError, ValueError):
        return float(d)


def _c(p, k, d):
    v = p.get(k)
    if isinstance(v, list) and len(v) >= 3:
        return np.array([float(x) for x in v[:3]], np.float64)
    return np.array(d, np.float64)


# ------------------------------------------------------------------------------------------------- noise
def _hash(ix, iy, iz, seed=0):
    h = (ix.astype(np.int64).astype(np.uint32) * np.uint32(0x8DA6B343)) \
        ^ (iy.astype(np.int64).astype(np.uint32) * np.uint32(0xD8163841)) \
        ^ (iz.astype(np.int64).astype(np.uint32) * np.uint32(0xCB1AB31F)) ^ np.uint32(seed * 0x9E3779B9 & 0xFFFFFFFF)
    h ^= h >> np.uint32(13)
    h *= np.uint32(0x5BD1E995)
    h ^= h >> np.uint32(15)
    return h


_GRAD = np.array([[1, 1, 0], [-1, 1, 0], [1, -1, 0], [-1, -1, 0], [1, 0, 1], [-1, 0, 1], [1, 0, -1], [-1, 0, -1],
                  [0, 1, 1], [0, -1, 1], [0, 1, -1], [0, -1, -1], [1, 1, 0], [-1, 1, 0], [0, -1, 1], [0, -1, -1]],
                 np.float64)


def noise3(x, y, z, seed=0):
    """Gradient noise, about -1..1, smooth (quintic fade)."""
    x0, y0, z0 = np.floor(x), np.floor(y), np.floor(z)
    fx, fy, fz = x - x0, y - y0, z - z0
    ix, iy, iz = x0.astype(np.int64), y0.astype(np.int64), z0.astype(np.int64)

    def fade(t):
        return t * t * t * (t * (t * 6 - 15) + 10)
    u, v, w = fade(fx), fade(fy), fade(fz)
    acc = None
    for dx in (0, 1):
        for dy in (0, 1):
            for dz in (0, 1):
                g = _GRAD[(_hash(ix + dx, iy + dy, iz + dz, seed) & np.uint32(15)).astype(np.intp)]
                d = g[..., 0] * (fx - dx) + g[..., 1] * (fy - dy) + g[..., 2] * (fz - dz)
                wgt = (u if dx else 1 - u) * (v if dy else 1 - v) * (w if dz else 1 - w)
                acc = d * wgt if acc is None else acc + d * wgt
    return acc


def _rand01(*ints, seed=0):
    a = [np.asarray(i) for i in ints] + [np.zeros_like(np.asarray(ints[0]))] * (3 - len(ints))
    return (_hash(a[0], a[1], a[2], seed) & np.uint32(0xFFFFFF)).astype(np.float64) / float(0x1000000)


# ------------------------------------------------------------------------------------------------- the sky
# The constants below are not the shader's (its code is not part of the port): they were set by eye against the
# shader's own preset icons, which are mental ray renders of the three sky scenes at their first and last key
# (re/tools/sky1/calib.sh).
BETA_R = 2.0 * np.array([5.8e-6, 13.5e-6, 33.1e-6])   # Rayleigh scattering at density 1, per metre (red, green, blue)
EXPOSURE = 1.0                                    # radiance scale of the in-scattered light
SUN_PATH = 0.03                                   # share of the optical depth towards the sun that dims the light
MOON_ALBEDO = 0.08                                # moon radiance per unit of sun intensity, fully lit
MOON_SIZE = 0.4                                   # the moon's diameter in degrees per unit of moon_size
GROUND_FADE = 0.05                                # radians below the horizon over which the ground darkens
CLOUD_AMBIENT = 0.004                             # cloud brightness per unit of sun intensity, unlit side
BETA_M = 1.5e-4                                   # Mie (dust) scattering per metre and unit of dust density
MIE_G = 0.8
NOISE_GAIN = 2.0                                  # gradient noise to about -1 .. 1


def _sphere_exit(r0, mu, R):
    """Distance along a ray from radius r0 (cosine mu to the vertical) to the sphere of radius R it is inside of."""
    disc = r0 * r0 * (mu * mu - 1.0) + R * R
    return -r0 * mu + np.sqrt(np.maximum(disc, 0.0))


def _ground_hit(r0, mu, R):
    """Distance to the planet (radius R) along the ray, or inf where it misses."""
    disc = r0 * r0 * (mu * mu - 1.0) + R * R
    d = -r0 * mu - np.sqrt(np.maximum(disc, 0.0))
    return np.where((disc >= 0) & (mu < 0) & (d > 0), d, np.inf)


class Sky:
    """Env_Atmosphere at one time: `radiance(dirs)` for unit directions in Softimage's world space (Y up)."""

    def __init__(self, p):
        self.p = p
        self.R = max(_f(p, "planet_radius", 6.0e6), 1.0e4)
        self.h = max(_f(p, "view_altitude", 1.8), 0.5)
        self.HR = max(_f(p, "atmosphere_thickness", 7400.0), 50.0)
        self.HM = max(_f(p, "dust_thickness", 1200.0), 10.0)
        self.bR = BETA_R * max(_f(p, "atmosphere_density", 1.0), 0.0)
        self.bM = BETA_M * max(_f(p, "dust_density", 0.01), 0.0)
        self.top = self.R + 8.0 * max(self.HR, self.HM)
        self.gain = _f(p, "view_gain", 1.0)
        self.sun_i = _f(p, "sun_intensity", 10.0) * _c(p, "sun_col", [1.0, 0.886, 0.662])
        self.sun = self.direction(_f(p, "sun_inclination", 30.0), _f(p, "sun_azimuth", -30.0))
        self.moon = self.direction(_f(p, "moon_inclination", 85.0), _f(p, "moon_azimuth", -100.0))
        self.samples = int(min(max(_f(p, "no_samples", 10), 12), 32))
        self._grid()

    @staticmethod
    def direction(inclination, azimuth):
        """Inclination: degrees from the vertical (+Y); azimuth: in the X-Z plane, 0 along +X, 90 along +Z."""
        i, a = math.radians(inclination), math.radians(azimuth)
        return np.array([math.sin(i) * math.cos(a), math.cos(i), math.sin(i) * math.sin(a)])

    # -- optical depth (Rayleigh, Mie) from points to the sun; inf where the planet is in the way
    def _sun_depth(self, r, mu_s, n=8):
        far = _sphere_exit(r, mu_s, self.top)
        blocked = np.isfinite(_ground_hit(r, mu_s, self.R))
        dR = np.zeros_like(r)
        dM = np.zeros_like(r)
        for k in range(n):
            s = far * (k + 0.5) / n
            rr = np.sqrt(r * r + s * s + 2 * r * s * mu_s)
            alt = np.maximum(rr - self.R, 0.0)
            dR += np.exp(-alt / self.HR) * far / n
            dM += np.exp(-alt / self.HM) * far / n
        dR = np.where(blocked, np.inf, dR)
        dM = np.where(blocked, np.inf, dM)
        return dR, dM

    def _grid(self):
        """The scattering integrals over view elevation x relative azimuth to the sun (the atmosphere is symmetric about
        the vertical plane through the sun): SR, SM (RGB, without the phase functions) and the transmittance T."""
        ne, na = 192, 48
        xs = np.linspace(-1.0, 1.0, ne)
        sin_e = np.sign(xs) * xs * xs                  # dense near the horizon
        cos_e = np.sqrt(np.maximum(1 - sin_e * sin_e, 0.0))
        dphi = np.linspace(0.0, math.pi, na)
        sun_az = math.atan2(self.sun[2], self.sun[0])
        az = sun_az + dphi
        E, A = np.meshgrid(np.arange(ne), np.arange(na), indexing="ij")
        d = np.stack([cos_e[E] * np.cos(az[A]), sin_e[E], cos_e[E] * np.sin(az[A])], -1).reshape(-1, 3)
        r0 = self.R + self.h
        mu = d[:, 1]
        L = np.minimum(_sphere_exit(r0, mu, self.top), _ground_hit(r0, mu, self.R))
        n = self.samples
        SR = np.zeros((len(d), 3))
        SM = np.zeros((len(d), 3))
        tR = np.zeros(len(d))
        tM = np.zeros(len(d))
        prev = np.zeros(len(d))
        for k in range(n):
            s1 = L * ((k + 1) / n) ** 2              # samples denser near the eye
            ds = s1 - prev
            s = prev + ds * 0.5
            prev = s1
            px = d * s[:, None]
            px[:, 1] += r0
            rr = np.linalg.norm(px, axis=1)
            alt = np.maximum(rr - self.R, 0.0)
            rhoR = np.exp(-alt / self.HR)
            rhoM = np.exp(-alt / self.HM)
            tR += rhoR * ds * 0.5
            tM += rhoM * ds * 0.5
            mu_s = (px @ self.sun) / rr
            sR, sM = self._sun_depth(rr, mu_s)
            with np.errstate(invalid="ignore"):
                tau = (self.bR[None, :] * (tR + SUN_PATH * sR)[:, None]
                       + 1.1 * self.bM * (tM + SUN_PATH * sM)[:, None])
            att = np.exp(-np.nan_to_num(tau, nan=np.inf, posinf=np.inf))
            SR += (rhoR * ds)[:, None] * att
            SM += (rhoM * ds)[:, None] * att
            tR += rhoR * ds * 0.5
            tM += rhoM * ds * 0.5
        T = np.exp(-(self.bR[None, :] * tR[:, None] + 1.1 * self.bM * tM[:, None]))
        self.ne, self.na = ne, na
        self.gSR = SR.reshape(ne, na, 3)
        self.gSM = SM.reshape(ne, na, 3)
        self.gT = T.reshape(ne, na, 3)
        self.sun_az = sun_az

    def _lookup(self, dirs, *grids):
        sin_e = np.clip(dirs[:, 1], -1.0, 1.0)
        x = np.sign(sin_e) * np.sqrt(np.abs(sin_e))
        fe = (x + 1.0) * 0.5 * (self.ne - 1)
        az = np.arctan2(dirs[:, 2], dirs[:, 0]) - self.sun_az
        dphi = np.abs((az + math.pi) % (2 * math.pi) - math.pi)
        fa = dphi / math.pi * (self.na - 1)
        e0 = np.clip(np.floor(fe).astype(np.intp), 0, self.ne - 2)
        a0 = np.clip(np.floor(fa).astype(np.intp), 0, self.na - 2)
        te = (fe - e0)[:, None]
        ta = (fa - a0)[:, None]
        out = []
        for g in grids:
            v = (g[e0, a0] * (1 - te) * (1 - ta) + g[e0 + 1, a0] * te * (1 - ta)
                 + g[e0, a0 + 1] * (1 - te) * ta + g[e0 + 1, a0 + 1] * te * ta)
            out.append(v)
        return out

    def transmittance(self, dirs):
        return self._lookup(dirs, self.gT)[0]

    def atmosphere(self, dirs):
        """In-scattered sunlight (linear, before the gain) and the transmittance to space along each direction."""
        SR, SM, T = self._lookup(dirs, self.gSR, self.gSM, self.gT)
        mu = np.clip(dirs @ self.sun, -1.0, 1.0)[:, None]
        pR = 3.0 / (16 * math.pi) * (1 + mu * mu) * EXPOSURE
        g = MIE_G
        pM = 3.0 / (8 * math.pi) * EXPOSURE * (1 - g * g) * (1 + mu * mu) / ((2 + g * g) * np.power(1 + g * g - 2 * g * mu, 1.5))
        L = self.sun_i[None, :] * (self.bR[None, :] * SR * pR + self.bM * SM * pM)
        return L, T

    # -- the bodies in front of the black of space
    def sun_disc(self, dirs, T, pix):
        p = self.p
        r = math.radians(max(_f(p, "sun_size", 5.0), 0.01) * 0.5)
        halo = max(_f(p, "sun_halo_size", 0.0), 0.0)
        ang = np.arccos(np.clip(dirs @ self.sun, -1.0, 1.0))
        soft = max(r * halo * 4.0, pix)
        disc = np.clip((r + soft * 0.5 - ang) / soft, 0.0, 1.0)
        glow = np.exp(-np.maximum(ang - r, 0.0) / max(r * halo * 3.0, 1e-6)) * (halo > 0) * 0.35
        k = (1.0 - min(max(_f(p, "sun_moderate", 0.0), 0.0), 1.0)) * (disc + glow)
        return self.sun_i[None, :] * k[:, None] * T

    def moon_disc(self, dirs, T, pix):
        p = self.p
        r = math.radians(max(_f(p, "moon_size", 5.0), 0.0) * MOON_SIZE * 0.5)
        col = _c(p, "moon_col", [0.8, 0.8, 0.8])
        if r <= 0 or not col.any():
            return 0.0
        halo = max(_f(p, "moon_halo_size", 0.1), 0.0)
        c = np.clip(dirs @ self.moon, -1.0, 1.0)
        ang = np.arccos(c)
        x = np.clip(np.sin(ang) / max(math.sin(r), 1e-9), 0.0, 1.0)
        tang = dirs - c[:, None] * self.moon[None, :]
        tl = np.linalg.norm(tang, axis=1)[:, None]
        tang = np.where(tl > 1e-12, tang / np.maximum(tl, 1e-12), 0.0)
        n = -np.sqrt(np.maximum(1 - x * x, 0.0))[:, None] * self.moon[None, :] + x[:, None] * tang
        lit = np.clip(n @ self.sun, 0.0, None)
        inside = np.clip((r + pix * 0.5 - ang) / max(pix, 1e-9), 0.0, 1.0) * (c > 0)
        base = self.sun_i[None, :] * col[None, :] * MOON_ALBEDO
        # the halo: scattered light of the lit part, falling off over `halo` moon radii
        phase = max(float(self.sun @ -self.moon) * 0.5 + 0.5, 0.0)
        glow = np.exp(-np.maximum(ang - r, 0.0) / max(r * halo * 3.0, 1e-6)) * (halo > 0) * 0.15 * (1 - phase) * (c > 0)
        return (base * (lit * inside + glow)[:, None]) * T

    def stars(self, dirs, T, pix, t):
        p = self.p
        dens = min(max(_f(p, "star_density", 0.6), 0.0), 1.0)
        bright = _f(p, "star_bright_scale", 1.0)
        if dens <= 0 or bright <= 0:
            return 0.0
        bias = _f(p, "star_bias", 0.1)
        vari = min(max(_f(p, "star_bright_vari", 0.5), 0.0), 1.0)
        sat = min(max(_f(p, "star_saturation", 0.2), 0.0), 1.0)
        rot = math.radians(_f(p, "star_rotation", 0.0))
        sx, sy = max(_f(p, "star_x_scale", 4.0), 0.1), max(_f(p, "star_y_scale", 4.0), 0.1)
        ox, oy = _f(p, "star_x_offset", 0.0), _f(p, "star_y_offset", 0.0)
        stime = _f(p, "star_time", 0.0)
        # cells on the sphere: azimuth x inclination, `cell` degrees at the scale 4 (a few hundred stars per steradian)
        az = np.arctan2(dirs[:, 2], dirs[:, 0]) + rot
        inc = np.arccos(np.clip(dirs[:, 1], -1.0, 1.0))
        cell = math.radians(0.9) * sx / 4.0, math.radians(0.9) * sy / 4.0
        u = az / cell[0] + ox
        v = inc / cell[1] + oy
        iu, iv = np.floor(u), np.floor(v)
        has = _rand01(iu, iv, seed=1) < dens * 0.5
        cu = iu + 0.2 + 0.6 * _rand01(iu, iv, seed=2)
        cv = iv + 0.2 + 0.6 * _rand01(iu, iv, seed=3)
        du = (u - cu) * cell[0] * np.sin(np.maximum(inc, 1e-3))
        dv = (v - cv) * cell[1]
        dist = np.sqrt(du * du + dv * dv)
        size = math.radians(0.04)
        sigma = max(size, pix * 0.6)
        prof = np.exp(-0.5 * (dist / sigma) ** 2) * (size / sigma) ** 2
        lum = bias + (1 - bias) * _rand01(iu, iv, seed=4) ** 3
        twinkle = 1.0 - vari * (0.5 + 0.5 * np.sin(stime * 6.0 + 40.0 * _rand01(iu, iv, seed=5)))
        hue = _rand01(iu, iv, seed=6)
        tint = np.stack([1 + sat * (hue - 0.5), np.ones_like(hue), 1 - sat * (hue - 0.5)], -1)
        s = (bright * lum * twinkle * prof * has)[:, None] * tint
        return s * T

    def clouds(self, dirs, pix, t):
        """Coverage (0..1) and distance (m) of the cloud layer along each direction (coverage 0 below the horizon)."""
        p = self.p
        amp = _f(p, "cloud_amplitude", 0.7)
        centre = _f(p, "cloud_centre", 0.3)
        if amp <= 0 and centre <= 0:
            return None, None
        ratio = _f(p, "cloud_ratio", 0.707)
        iters = max(_f(p, "cloud_iterations", 5.0), 0.0)
        alt = max(_f(p, "cloud_altitude", 500.0), 10.0)
        xs, ys = max(_f(p, "cloud_x_scale", 1.0), 1e-3), max(_f(p, "cloud_y_scale", 1.0), 1e-3)
        xo, yo = _f(p, "cloud_x_offset", 0.0), _f(p, "cloud_y_offset", 0.0)
        rot = math.radians(_f(p, "cloud_rotation", 0.0))
        ctime = _f(p, "cloud_time", 0.0) if int(_f(p, "cloud_animate", 0)) else 0.0
        turb = bool(int(_f(p, "cloud_absolute", 0)))
        r0 = self.R + self.h
        up = dirs[:, 1] > 1e-4
        dist = np.where(up, _sphere_exit(r0, dirs[:, 1], self.R + alt), np.inf)
        # the hit point in the lollipop map around the view point (at the pole): arc length (km) and direction
        hx = dirs[:, 0] * np.where(up, dist, 0.0)
        hz = dirs[:, 2] * np.where(up, dist, 0.0)
        hy = dirs[:, 1] * np.where(up, dist, 0.0) + r0
        horiz = np.sqrt(hx * hx + hz * hz)
        arc = np.arctan2(horiz, hy) * (self.R + alt) / 1000.0
        psi = np.arctan2(hz, hx)
        X, Z = arc * np.cos(psi), arc * np.sin(psi)
        X, Z = X * math.cos(rot) - Z * math.sin(rot), X * math.sin(rot) + Z * math.cos(rot)
        X, Z = X / xs + xo, Z / ys + yo
        # footprint of a pixel on the layer (in fractal units), to fade out octaves finer than a pixel
        foot = dist * pix / np.maximum(dirs[:, 1], 1e-3) / 1000.0 / min(xs, ys)
        # the fractal: octaves of noise, each `ratio` times the one before and twice its frequency, a fractional last
        # octave in part (so an iteration count under 1 gives faint, smooth clouds)
        total, a, f = 0.0, 1.0, 1.0
        for k in range(int(math.ceil(iters))):
            part = min(iters - k, 1.0)
            w = np.clip(1.0 - (f * foot - 0.25) / 0.25, 0.0, 1.0) * part * a
            nz = noise3(X * f, Z * f, np.full_like(X, ctime * 0.5 + 17.3 * k), seed=11 + k) * NOISE_GAIN
            if turb:
                nz = np.abs(nz) * 2 - 0.6
            total = total + w * nz
            a *= ratio
            f *= 2.0
        # the cloud where the fractal is over a half, thickening to full cover at 1
        F = centre + amp * total
        cov = np.where(up, np.clip(2.0 * F - 1.0, 0.0, 1.0), 0.0)
        return cov, dist

    def radiance(self, dirs, pix=0.002, t=0.0):
        """Linear radiance (RGB, before the gain) along unit directions (N, 3)."""
        L, T = self.atmosphere(dirs)
        # below the horizon: the planet hides what lies beyond it; the ground map is empty in every scene of the corpus,
        # so the ground is the haze of the horizon above it, darkening over GROUND_FADE radians of depression
        ground = ~np.isinf(_ground_hit(self.R + self.h, dirs[:, 1], self.R))
        if ground.any():
            dh = dirs[ground].copy()
            dh[:, 1] = 1e-3
            dh /= np.linalg.norm(dh, axis=1, keepdims=True)
            fade = np.exp(-np.maximum(-dirs[ground, 1], 0.0) / GROUND_FADE)[:, None]
            L[ground] = self.atmosphere(dh)[0] * fade + L[ground] * (1.0 - fade)
        T = T * (~ground)[:, None]
        # the stars fade out as the sky brightens (they are lost in the glare of the day)
        sky_lum = (L * self.gain).mean(axis=1, keepdims=True)
        fade = np.clip(1.0 - sky_lum * 6.0, 0.0, 1.0)
        L = L + self.stars(dirs, T, pix, t) * fade + self.moon_disc(dirs, T, pix)
        cov, dist = self.clouds(dirs, pix, t)
        if cov is not None:
            col = _c(self.p, "cloud_col", [1.0, 1.0, 1.0])
            alt = max(_f(self.p, "cloud_altitude", 500.0), 10.0)
            r = self.R + alt
            sR, sM = self._sun_depth(np.array([r]), np.array([float(self.sun[1])]), n=16)
            Ts = np.exp(-SUN_PATH * (self.bR * sR[0] + 1.1 * self.bM * sM[0])) if np.isfinite(sR[0]) else np.zeros(3)
            amb = self.atmosphere(np.array([[0.0, 1.0, 0.0], [0.0, 0.2, 0.98]]))[0].mean(axis=0)
            mu = np.clip(dirs @ self.sun, -1.0, 1.0)
            # lit from above by the sun (more towards the sun: forward scattering, thinner edges brighter)
            lit = 0.02 + 0.10 * np.power(np.clip(mu, 0.0, 1.0), 8) * (1.0 - cov) * 4.0
            C = col[None, :] * (self.sun_i[None, :] * Ts[None, :] * (lit[:, None] * 0.6 + CLOUD_AMBIENT)
                                + amb[None, :] * 1.5)
            ext = self.bR * np.exp(-alt * 0.5 / self.HR) + 1.1 * self.bM * np.exp(-alt * 0.5 / self.HM)
            Tv = np.exp(-ext[None, :] * np.minimum(dist, 1e9)[:, None])
            c = cov[:, None]
            L = L * (1.0 - c * Tv) + c * C * Tv
            T = T * (1.0 - c)
        L = L + self.sun_disc(dirs, T, pix)
        return L

    def display(self, dirs, pix=0.002, t=0.0):
        """Display values 0..1 (the shader's colour times the gain, clipped as a picture holds it)."""
        return np.clip(self.radiance(dirs, pix, t) * self.gain, 0.0, 1.0)


def latlong_dirs(w, h):
    """Directions of a latitude-longitude map as the engine's reflection-map look-up reads it (si-render shade.rs
    `reflect`: u = (atan2(z, x) / pi + 1) / 2, v = acos(y) / pi), row 0 of the returned grid first."""
    u = (np.arange(w) + 0.5) / w
    v = (np.arange(h) + 0.5) / h
    az = (2 * u - 1) * math.pi
    inc = v * math.pi
    A, I = np.meshgrid(az, inc)
    return np.stack([np.sin(I) * np.cos(A), np.cos(I), np.sin(I) * np.sin(A)], -1).reshape(-1, 3)


def sky_map(sky, t, size=ENV_SIZE, top_first=True):
    """The sky as an 8-bit latitude-longitude map (h, w, 3), its first row the zenith when `top_first`."""
    w, h = size
    d = latlong_dirs(w, h)
    img = sky.display(d, pix=math.pi / h, t=t).reshape(h, w, 3)
    if not top_first:
        img = img[::-1]
    return np.rint(img * 255).astype(np.uint8)


def sky_behind(sky, dirs_rows, t, pix):
    """The sky for a picture: `dirs_rows` (h, w, 3) camera rays in Softimage's world space; 8-bit (h, w, 3)."""
    h, w, _ = dirs_rows.shape
    out = np.empty((h, w, 3), np.uint8)
    step = max(1, 400000 // max(w, 1))
    for y in range(0, h, step):
        d = dirs_rows[y:y + step].reshape(-1, 3)
        d = d / np.linalg.norm(d, axis=1, keepdims=True)
        out[y:y + step] = np.rint(sky.display(d, pix, t) * 255).reshape(-1, w, 3).astype(np.uint8)
    return out


def over(planes_rgba, sky_rgb):
    """The picture (premultiplied, uint8 (h, w, 4)) over the sky: v + (1 - a) * sky, alpha 255."""
    a = planes_rgba[..., 3:4].astype(np.float64) / 255.0
    v = planes_rgba[..., :3].astype(np.float64) + (1.0 - a) * sky_rgb.astype(np.float64)
    out = np.empty_like(planes_rgba)
    out[..., :3] = np.clip(np.rint(v), 0, 255).astype(np.uint8)
    out[..., 3] = 255
    return out


# ------------------------------------------------------------------------------------------------- water
def water(p, t, size=WATER_SIZE):
    """The Water texture shader over the unit square at time `t` (seconds): RGBA floats (size, size, 4), row 0 at v = 1
    (the top of the picture), u along the columns."""
    n = int(min(max(_f(p, "no_waves", 10), 1), 64))
    vel = _f(p, "wave_vel", 0.3)
    f0 = max(_f(p, "wave_freq", 5.0), 1e-3)
    amp = _f(p, "wave_amp", 0.2)
    sub = max(_f(p, "wave_sub_freq", 0.2), 0.0)
    smooth = max(_f(p, "smoothness", 2.0), 0.0)
    wdir = _f(p, "wind_dir", 0.0)
    wdev = _f(p, "wind_dev", 0.0)
    tt = t + _f(p, "wave_offset", 0.0)
    ur, vr = _f(p, "urange", 1.0), _f(p, "vrange", 1.0)
    u = (np.arange(size) + 0.5) / size * ur
    v = (1.0 - (np.arange(size) + 0.5) / size) * vr
    U, V = np.meshgrid(u, v)
    h = np.zeros_like(U)
    k_ = np.arange(n)
    dev = (_rand01(k_, seed=21) - 0.5) * wdev
    ph = _rand01(k_, seed=22) * 2 * math.pi
    for k in range(n):
        f = f0 * (1.0 + sub * k)
        a = amp * (f0 / f) ** smooth
        ang = math.radians(wdir + float(dev[k]))
        c = vel * math.sqrt(f0 / f)             # deep water: the phase speed goes as the square root of the wavelength
        h += a * np.sin(2 * math.pi * f * (U * math.cos(ang) + V * math.sin(ang) - c * tt) + float(ph[k]))
    s = 0.5 + h
    bias = min(max(_f(p, "col_bias", 0.5), 1e-3), 1 - 1e-3)
    if int(_f(p, "col_clip", 1)):
        s = np.clip(s, 0.0, 1.0)
    s = np.where(s > 0, np.power(np.maximum(s, 0.0), math.log(bias) / math.log(0.5)), s)
    tr = p.get("col_trough") if isinstance(p.get("col_trough"), list) else [0.0, 0.0, 0.3, 0.0]
    cr = p.get("col_crest") if isinstance(p.get("col_crest"), list) else [1.0, 1.0, 1.0, 1.0]
    tr = np.array((list(tr) + [0.0] * 4)[:4], np.float64)
    cr = np.array((list(cr) + [1.0] * 4)[:4], np.float64)
    img = tr[None, None, :] + (cr - tr)[None, None, :] * s[..., None]
    return np.clip(img, 0.0, 1.0)


# ------------------------------------------------------------------------------------------------- import (bpy)
def import_shaders(builder):
    """At import (importer.Builder.run): sky domes flagged and kept out of the render, the sky stored on the scene, a
    sky reflection map on the reflective materials without one, Water pictures generated.  Nothing happens for a
    scene without these shaders.  Returns notes."""
    import bpy
    d = builder.d
    shaders = d.get("shaders") or []
    if not shaders:
        return []
    notes = []
    rate = float(d["settings"].get("frame_rate") or 30.0) or 30.0
    first = int(d["settings"].get("frame_first", 1))
    by_id = {s["id"]: s for s in shaders}

    def ref_shader(r):
        if isinstance(r, dict) and r.get("active", True) and r.get("shader") is not None:
            return by_id.get(r["shader"])
        return None
    sky_shader = None
    sky_mats = set()
    for m in d.get("materials", []):
        s = ref_shader((m.get("shaders") or {}).get("material"))
        if s is not None and library(s) == ATMOSPHERE:
            sky_mats.add(m["id"])
            sky_shader = sky_shader or s
    sc = builder.context.scene
    if sky_shader is not None:
        for m in builder.models:
            ob = builder.objects.get(m["id"])
            if ob is not None and any(i in sky_mats for i in (m.get("materials") or [])):
                ob[DOME_PROP] = sky_shader.get("element", "")
                ob.hide_render = True
                ob.display_type = "WIRE"
        sc[SKY_PROP] = json.dumps({"shader": sky_shader, "rate": rate})
        # under mental ray every ray that misses the scene hits the dome, so the picture is opaque: the sky is the
        # background (the importer makes Film > Transparent on for a Softimage picture; ticked again, it drops the sky)
        sc.render.film_transparent = False
        notes.append("mental ray %s (%s): approximated (sky behind the picture and as a reflection map; the dome is "
                     "not rendered)" % (library(sky_shader), sky_shader.get("udf")))
        img = _sky_image(sc, sky_shader, rate, first)
        # what a reflected ray sees: under mental ray it hits the dome, so the sky.  A reflective material without a
        # reflection map gets the sky as one (rays first, the map where nothing is hit); a map that comes after the
        # traced rays (method 7) showed only the dome and now shows the sky; a map instead of rays (method 8) is kept
        n = swapped = 0
        for (mid, own, t3), mat in builder.blender_materials.items():
            m = builder.materials.get(mid) or {}
            if mid in sky_mats:
                continue
            rm = m.get("reflection_map")
            if rm is not None:
                if (builder.textures.get(rm) or {}).get("method") == 7:
                    swapped += _swap_texture_image(builder, rm, img, "Env_Atmosphere", mats=[mat])
                continue
            if float(m.get("reflectivity", 0.0)) <= 0.0:
                continue
            if _add_sky_map(builder, mat, img):
                n += 1
        if n:
            notes.append("sky reflection map put on %d reflective material(s)" % n)
        if swapped:
            notes.append("sky in place of %d reflection map picture(s) behind traced reflections" % swapped)
    for t in d.get("textures", []):
        s = ref_shader(t.get("shader"))
        if s is None or library(s) != WATER:
            continue
        img = builder.images.get(t["id"])
        wimg = _water_image(s, rate, first)
        _swap_texture_image(builder, t["id"], wimg, "Water")
        notes.append("mental ray Water (%s) on texture %d: generated picture, animated per frame" % (s.get("udf"), t["id"]))
    # the other shaders the scene uses (an active reference), and the ones it only carries switched off
    refs = [r for m in d.get("materials", []) for r in (m.get("shaders") or {}).values()]
    refs += [t.get("shader") for t in d.get("textures", []) + d.get("textures3d", []) + (d.get("lights") or [])]
    ss = d.get("scene_shaders") or {}
    refs += [ss.get("atmosphere")] + list(ss.get("output") or []) + list(ss.get("lens") or [])
    used = {library(s) for s in (ref_shader(r) for r in refs) if s is not None}
    others = sorted(used - {ATMOSPHERE, WATER})
    if others:
        notes.append("mental ray shaders not approximated: " + ", ".join(others))
    off = sorted({library(s) for s in shaders} - used - {ATMOSPHERE, WATER})
    if off:
        notes.append("mental ray shaders switched off in the scene (not used): " + ", ".join(off))
    return notes


def _store(img, rgba_top_first):
    """Puts an (h, w, 4) float picture (row 0 the top) into a byte image and packs it (so a saved .blend keeps it)."""
    h, w = rgba_top_first.shape[:2]
    img.pixels.foreach_set(np.ascontiguousarray(rgba_top_first[::-1], np.float32).ravel())
    try:
        img.pack()
    except RuntimeError:
        pass


def _sky_image(sc, shader, rate, frame):
    import bpy
    name = "mental ray sky %s" % shader.get("udf", "")
    w, h = ENV_SIZE
    img = bpy.data.images.get(name) or bpy.data.images.new(name, w, h, alpha=True)
    sky = Sky(params_at(shader, frame / rate))
    m = sky_map(sky, frame / rate).astype(np.float32) / 255.0
    _store(img, np.concatenate([m, np.ones((h, w, 1), np.float32)], -1))
    img[SKYMAP_PROP] = 1
    return img


def _water_image(shader, rate, frame):
    import bpy
    name = "mental ray water %s" % shader.get("udf", "")
    img = bpy.data.images.get(name) or bpy.data.images.new(name, WATER_SIZE, WATER_SIZE, alpha=True)
    _store(img, water(params_at(shader, frame / rate), frame / rate).astype(np.float32))
    img[WATER_PROP] = json.dumps({"shader": shader, "rate": rate})
    return img


def _texture_nodes(builder):
    for mat in builder.blender_materials.values():
        tree = getattr(mat, "node_tree", None)
        if tree is None:
            continue
        for n in tree.nodes:
            if n.get("si3d_texture_id") is not None:
                yield mat, n


def _swap_texture_image(builder, tid, img, shader, mats=None):
    """The picture of the Softimage Texture nodes of texture `tid` (in `mats`, or every material) replaced by `img`;
    returns how many."""
    k = 0
    for mat, n in _texture_nodes(builder):
        if mats is not None and mat not in mats:
            continue
        if n.get("si3d_texture_id") == tid and hasattr(n, "image"):
            n.image = img
            n["si3d_mr_shader"] = shader
            if hasattr(n, "crop"):
                n.crop = False
            k += 1
    return k


def _add_sky_map(builder, mat, img):
    """A Reflection map (method 7) texture node with the sky on the material's Softimage node, when it has a free
    texture input."""
    mn = builder.mn
    if mn is None:
        return False
    node = mn.material_node(mat)
    if node is None:
        return False
    try:
        two, _ = mn.textures_of(node)
    except Exception:   # noqa: BLE001
        return False
    if len(two) >= 4:
        return False
    vals = {"method": 7, "pixel_interp": 1, "mask": 1, "channel": 2, "uscale": 1.0, "vscale": 1.0, "uoffset": 0.0,
            "voffset": 0.0, "urepeat": 1, "vrepeat": 1, "blend": 1.0, "ambient": 0.0, "diffuse": 0.0, "specular": 0.0,
            "reflectivity": 0.0, "transparency": 0.0, "roughness": 0.0, "reflection_map": 1.0, "alt_u": 0, "alt_v": 0,
            "swap_uv": 0, "black_white": 0}
    tn = mn.new_texture_node(node, img, vals, scope="LOCAL")
    tn["si3d_mr_shader"] = "Env_Atmosphere"
    tn.label = "mental ray sky (reflection map)"
    try:
        mn.refresh(mat.node_tree)
    except Exception:   # noqa: BLE001
        pass
    return True


# ------------------------------------------------------------------------------------------------- render (bpy)
def _safe(name):
    return "".join(ch if ch.isalnum() else "_" for ch in name) or "image"


def _write_raw(path, rgb_or_rgba_top_first):
    import struct
    a = np.ascontiguousarray(rgb_or_rgba_top_first, np.uint8)
    h, w, planes = a.shape
    with open(path, "wb") as fh:
        fh.write(b"SIRAW" + struct.pack("<III", w, h, planes))
        fh.write(a.tobytes())


def render_textures(scene, texdir, frame):
    """Before the database is built: the generated pictures of this frame written over the ones scene_to_dict wrote
    (the water at the frame's time, the sky map with the sun, moon and clouds where they are at that time)."""
    import bpy
    if not texdir or not os.path.isdir(texdir):
        return
    sky_json = scene.get(SKY_PROP)
    for img in bpy.data.images:
        path = os.path.join(texdir, "%s.raw" % _safe(img.name))
        if not os.path.isfile(path):
            continue
        if img.get(WATER_PROP):
            w = json.loads(img[WATER_PROP])
            t = frame / float(w.get("rate") or 30.0)
            pic = water(params_at(w["shader"], t), t, max(img.size[0], 16))
            _write_raw(path, np.rint(pic * 255).astype(np.uint8))
        elif img.get(SKYMAP_PROP) and sky_json:
            s = json.loads(sky_json)
            t = frame / float(s.get("rate") or 30.0)
            sky = Sky(params_at(s["shader"], t))
            _write_raw(path, sky_map(sky, t, (img.size[0], img.size[1])))


def camera_rays(cam, scene, w, h, box=None):
    """Camera rays (h, w, 3) in Softimage's world space (row 0 the top of the picture) and the angle of a pixel, for the
    (evaluated) camera object; `box` (x0, y0, bw, bh, pixels of the full frame, y from the bottom) when the picture is a
    render region's."""
    r = scene.render
    rw = int(r.resolution_x * r.resolution_percentage / 100)
    rh = int(r.resolution_y * r.resolution_percentage / 100)
    tr, br, bl, tl = [np.array(v[:], np.float64) for v in cam.data.view_frame(scene=scene)]
    x0, y0, bw, bh = box if box else (0, 0, rw, rh)
    xs = (x0 + (np.arange(w) + 0.5) * bw / w) / rw
    ys = (y0 + (np.arange(h)[::-1] + 0.5) * bh / h) / rh      # row 0 the top
    X, Y = np.meshgrid(xs, ys)
    d = bl[None, None, :] + X[..., None] * (br - bl)[None, None, :] + Y[..., None] * (tl - bl)[None, None, :]
    if cam.data.type == "ORTHO":
        d = np.broadcast_to(np.array([0.0, 0.0, -1.0]), d.shape).copy()
        pix = 1e-3
    else:
        pix = float(np.linalg.norm(tl - bl)) / max(rh, 1) / max(abs(float(tl[2])), 1e-9)
    m = np.array(cam.matrix_world.to_3x3().normalized())
    dw = d @ m.T
    si = np.stack([dw[..., 0], dw[..., 2], -dw[..., 1]], -1)      # Blender (x, y, z) = Softimage (x, -z, y)
    return si / np.linalg.norm(si, axis=-1, keepdims=True), pix


class Picture(tuple):
    """A decoded picture (w, h, planes) that keeps the path of the engine's .pic it was made from (`source`; None for
    woven fields), so that the Depth pass still reads the depth file beside it (__init__.py)."""
    source = None


def render_composite(depsgraph, pic, frame):
    """After the engine's frame: the sky put behind it (the picture is premultiplied: its colour plus (1 - alpha) times
    the sky; alpha 255 after).  `pic` a .pic path or decoded (w, h, planes); returns a Picture (w, h, planes), or None
    when the scene has no sky, the sky is switched off (switch_sky) or Film > Transparent is on (then nothing changes:
    the background stays transparent).  A render region's picture is either the box or the whole frame."""
    scene = depsgraph.scene
    s = scene.get(SKY_PROP)
    if not s or scene.camera is None or scene.render.film_transparent:
        return None
    if isinstance(pic, tuple):
        w, h, planes = pic
    else:
        from . import sipic
        with open(pic, "rb") as fh:
            w, h, planes = sipic.decode_np(fh.read())
    px = np.zeros((h, w, 4), np.uint8)
    for i, k in enumerate("RGBA"):
        p = planes.get(k)
        px[..., i] = (255 if k == "A" else 0) if p is None else np.asarray(p, np.uint8).reshape(h, w)
    r = scene.render
    box = None
    if r.use_border:
        from . import session
        rw = int(r.resolution_x * r.resolution_percentage / 100)
        rh = int(r.resolution_y * r.resolution_percentage / 100)
        box = session.region_fractions((r.border_min_x, r.border_max_x, r.border_min_y, r.border_max_y), rw, rh)[1]
        if (w, h) != tuple(box[2:]):
            box = None
    s = json.loads(s)
    t = frame / float(s.get("rate") or 30.0)
    sky = Sky(params_at(s["shader"], t))
    cam = scene.camera.evaluated_get(depsgraph)
    rays, pix = camera_rays(cam, scene, w, h, box)
    out = over(px, sky_behind(sky, rays, t, pix))
    res = Picture((w, h, {k: out[..., i].ravel().copy() for i, k in enumerate("RGBA")}))
    res.source = pic if isinstance(pic, str) else None
    return res


def switch_sky(scene, on):
    """The sky stand-in of an imported scene on or off.  Off: nothing is put behind the picture and the domes render
    again with their own Softimage material, as the Softimage engine shows the scene (the reflection maps keep the
    sky picture made at import), with Film > Transparent on as the importer leaves any scene; on: the sky behind the
    picture and Film > Transparent off.  Returns whether the scene has a sky at all."""
    s = scene.get(SKY_PROP) or scene.get(SKY_OFF_PROP)
    if not s:
        return False
    scene[SKY_PROP if on else SKY_OFF_PROP] = s
    k = SKY_OFF_PROP if on else SKY_PROP
    if k in scene:
        del scene[k]
    for ob in scene.objects:
        if ob.get(DOME_PROP) is not None:
            ob.hide_render = bool(on)
    scene.render.film_transparent = not on
    return True
