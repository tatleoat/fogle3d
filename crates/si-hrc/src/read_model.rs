// SPDX-License-Identifier: MIT
//! The reader of a model hierarchy and of the shapes and attachments of its nodes
//! (source file `binary.c`, with the chunk readers and helpers it calls in `dynamicio.c`,
//! `invkin.c`, `envtools.c`, `t2d_dial.c` and `metaball.c`).

use crate::io::ok;
use crate::model::*;
use crate::proto;
use crate::read::{below, is_custom_tag, Reader};


/// The wrap flags of a texture, from the shape it is applied to [soft 0x082b0788 updDoWrap].
/// On a closed patch or NURBS surface the UV method (4) wraps in the closed directions; on
/// a mesh it never wraps; on the shapes that have no UV space of their own it is replaced
/// by method 6.
fn upd_do_wrap(node: &Node, x: &mut Texture2D) {
    let (mut u, mut v) = (1i16, 1i16);
    match node.kind {
        1 => {
            if let Some(Definition::Patch(p)) = node.definitions.first() {
                u = p.close[0];
                v = p.close[1];
            }
        }
        10 => {
            if let Some(Definition::NurbsSurface(s)) = node.definitions.first() {
                u = s.closed[0];
                v = s.closed[1];
            }
        }
        4 => {
            if x.method == 4 {
                u = 0;
                v = 0;
            }
        }
        0 | 5 | 6 | 9 => {}
        _ => {
            if x.method == 4 {
                x.method = 6;
            }
        }
    }
    match x.method {
        1..=3 => {
            x.wrap_u = 0;
            x.wrap_v = 0;
        }
        4 => {
            x.wrap_u = u;
            x.wrap_v = v;
        }
        5..=8 => {
            x.wrap_u = 1;
            x.wrap_v = 0;
        }
        _ => {} // the original prints "Unknown texture surface or reflection type" and goes on
    }
}

/// Keeps the scale of a wrapping texture within −1 to 1 and away from zero
/// [soft 0x082ada24 SI_txt2dSetScale]. The comparisons are against double constants.
fn set_scale(x: &mut Texture2D) {
    fn clamp(s: f32) -> f32 {
        let d = s as f64;
        if d < -1.0 {
            -1.0
        } else if 1.0 < d {
            1.0
        } else if d > -0.0001 && d < 0.0001 {
            if 0.0 < d { 0.0001 } else { -0.0001 }
        } else {
            s
        }
    }
    if x.wrap_u != 0 {
        x.uscale = clamp(x.uscale);
    }
    if x.wrap_v == 0 && x.method != 6 && (x.method.wrapping_sub(7) as u16) > 1 {
        return;
    }
    x.vscale = clamp(x.vscale);
}

/// [soft 0x082ad9f8 SI_txt2dSetWrap]. Returns whether a stored value changed.
fn set_wrap(node: &Node, x: &mut Texture2D) -> bool {
    let before = (x.method, x.uscale.to_bits(), x.vscale.to_bits());
    upd_do_wrap(node, x);
    set_scale(x);
    before != (x.method, x.uscale.to_bits(), x.vscale.to_bits())
}

const WRAP_CHANGE: &str = "texture on a model: method or scale adjusted to the shape";

impl<'a> Reader<'a> {
    /// One node of the hierarchy and, through its son and brother tokens, everything below
    /// and beside it [soft 0x083eca24 modelBinaryReadNode] (the mode that builds a new
    /// hierarchy; `parent` is the node this one hangs under).
    ///
    /// The switch is on the whole 16-bit token, length byte included: 0x1a and 0x41a are
    /// two cases that happen to do the same, and a token with an unexpected length byte is
    /// simply unknown.
    ///
    /// Not reproduced, because they belong to other subsystems: the matrices the original
    /// derives from the transformation, the registration of a shape with the model beyond
    /// keeping it in the list, the resolution of metaball group names into models, and the
    /// propagation of the node's state to its sons.
    pub fn node(&mut self, mut parent: Option<&mut Node>) -> Box<Node> {
        let mut n = Box::new(Node::default());
        self.notes.nodes += 1;
        let mut i = 0usize;
        let mut t = self.tok(proto::HIERARCHY, &mut i);
        let mut tmp = 0i16;
        let mut external = 0i16;
        while t != 0 {
            match t as u16 {
                1 => n.name = self.r.string(),
                2 => {
                    self.r.i16(&mut tmp);
                    n.kind = tmp as i32;
                }
                3 => {
                    // the shape, by the node's type. For the types without one the original
                    // tests a pointer it never set; it is taken as null here.
                    let d = match n.kind {
                        1 => Some(Definition::Patch(self.patch())),
                        2 => Some(Definition::Face(self.face())),
                        4 => Some(Definition::Mesh(self.mesh(&mut n))),
                        6 => Some(Definition::Spline(self.spline())),
                        7 => Some(Definition::MetaSystem(self.metasystem())),
                        8 => Some(Definition::Metaball(self.metaball())),
                        9 => Some(Definition::NurbsCurve(self.nurbs_curve())),
                        10 => Some(Definition::NurbsSurface(self.nurbs_surface())),
                        -1 | 0 | 3 | 5 => None,
                        _ => {
                            self.r.skip_field(-1); // an unknown type: a length and its bytes
                            None
                        }
                    };
                    n.definitions.extend(d);
                }
                4 => ok(self.r.floats(&mut n.scaling)),
                5 => ok(self.r.floats(&mut n.rotation)),
                6 => ok(self.r.floats(&mut n.translation)),
                7 => {
                    self.r.i16(&mut n.nb_materials);
                    for _ in 0..self.count(n.nb_materials as i64) {
                        if self.ended() {
                            break;
                        }
                        self.r.i16(&mut external);
                        if external == 0 {
                            let mut m = self.material();
                            for x in m.textures.iter_mut() {
                                if set_wrap(&n, x) {
                                    self.changed(WRAP_CHANGE);
                                }
                            }
                            n.materials.push(Some(m));
                        } else {
                            self.r.string(); // the material's name; the slot stays null
                            n.materials.push(None);
                            self.changed("external texture or material name dropped");
                        }
                    }
                }
                8 => {
                    self.r.i16(&mut n.nb_textures);
                    for _ in 0..self.count(n.nb_textures as i64) {
                        if self.ended() {
                            break;
                        }
                        self.r.i16(&mut external);
                        if external == 0 {
                            let mut x = self.texture();
                            if set_wrap(&n, &mut x) {
                                self.changed(WRAP_CHANGE);
                            }
                            n.textures.push(x);
                        } else {
                            self.r.string();
                            self.changed("external texture or material name dropped");
                        }
                    }
                }
                9 => {
                    self.r.i16(&mut tmp);
                    for k in 0..self.count(tmp as i64) {
                        if self.ended() {
                            break;
                        }
                        self.r.i16(&mut external);
                        if external == 0 {
                            let x = self.texture3d();
                            if k == 0 {
                                n.texture3d = Some(x); // one that is not the first is freed
                            } else {
                                self.changed("3D texture after the first dropped");
                            }
                        } else {
                            self.r.string();
                            self.changed("external texture or material name dropped");
                        }
                    }
                }
                0xa => {
                    self.r.i16(&mut tmp);
                    if tmp != 0 {
                        n.son = Some(self.node(Some(&mut n)));
                    }
                }
                0xb => {
                    self.r.i16(&mut tmp);
                    if tmp != 0 {
                        n.brother = Some(self.node(parent.as_deref_mut()));
                    }
                }
                0xc => {
                    self.r.i16(&mut n.shape_interp);
                    if n.shape_interp == -1 {
                        n.shape_interp = 0;
                        self.changed("shape interpolation -1 read as 0");
                    }
                }
                0xd => {
                    // before 2.35: one chunk whose kind doubles as the animation algorithm
                    self.r.i16(&mut tmp);
                    n.algorithm = tmp as i32;
                    match tmp {
                        2 => {
                            self.old_dynamics(&mut n);
                            n.algorithm = 1;
                        }
                        1 => n.ik = Some(self.inv_kin()),
                        0xc => {
                            self.old_dynamics(&mut n);
                            n.algorithm = 0;
                        }
                        _ => ok(self.r.skip_field(-1)),
                    }
                }
                0xe => {
                    self.r.i16(&mut tmp);
                    if tmp != 0 {
                        self.r.i16(&mut external); // one more short, read and not used
                        n.dynamics = Some(self.dyna_matter());
                    }
                }
                0xf => {
                    self.r.i16(&mut tmp);
                    n.envelop_type = tmp as i32;
                    self.r.i16(&mut tmp);
                    if tmp != 0 {
                        n.envelop = Some(self.old_envelop());
                    }
                }
                0x10 => {
                    self.r.i16(&mut tmp);
                    n.algorithm = tmp as i32;
                    if tmp == 2 || tmp == 0xc {
                        self.changed("algorithm 2 or 12 (written as 1 or 0)");
                    }
                }
                0x11 => {
                    self.r.i16(&mut tmp);
                    n.envelop_type = tmp as i32;
                    self.r.i16(&mut tmp);
                    if tmp != 0 {
                        let e = self.envelop(&mut n, parent.as_deref_mut());
                        n.envelop = Some(e);
                        // before 3.49089 a chain root (a null with algorithm 1 or 2) loses
                        // the envelope it stored
                        if below(self.versions.binary, 3.49089) && n.kind == 0 && (n.algorithm.wrapping_sub(1) as u32) < 2 {
                            n.envelop = None;
                            self.changed("envelope of a chain root dropped (below 3.49089)");
                        }
                    }
                }
                0x12 => {
                    self.r.i16(&mut tmp);
                    if tmp != 0 {
                        n.chain_dynamics = true;
                    }
                }
                0x13 => {
                    self.r.i16(&mut tmp);
                    if tmp != 0 {
                        n.joint_friction = Some(self.joint_properties());
                    }
                }
                0x14 => {
                    // a flag of 0 allocates default inverse kinematics when the global
                    // SI_edSumatraTo3x is set (an import mode; taken as unset here)
                    self.r.i16(&mut tmp);
                    if tmp != 0 {
                        n.ik = Some(self.inv_kin());
                    }
                }
                0x15 => {
                    self.r.i16(&mut tmp);
                    for _ in 0..self.count(tmp as i64) {
                        if self.ended() {
                            break;
                        }
                        self.cluster(&mut n);
                    }
                }
                0x16 => self.user_nodes(&mut n),
                0x17 => ok(self.r.i16(&mut n.visibility)),
                0x18 => ok(self.r.i16(&mut n.motion_blur)),
                0x19 => {
                    self.r.i16(&mut tmp);
                    if tmp != 0 {
                        let it = n.init_transfo.get_or_insert([[0.0; 3]; 3]);
                        for v in it.iter_mut() {
                            self.r.floats(v);
                        }
                    }
                }
                0x1a | 0x41a => ok(self.r.f32(&mut n.matte)),
                0x1b | 0x21b => ok(self.r.i16(&mut n.garbage_matte)),
                0x221 => ok(self.r.i16(&mut n.x404)),
                0x222 => ok(self.r.i16(&mut n.x406)),
                0x223 => ok(self.r.i16(&mut n.x420)),
                0x224 => {
                    let mut f = 0i16;
                    self.r.i16(&mut f);
                    if f == 1 {
                        n.flag8 = true;
                    }
                }
                0xff1c => {
                    // user data in the newer form: appended to the same list as the old
                    self.r.i32(&mut 0);
                    let list = self.user_data();
                    n.user_nodes.extend(list);
                }
                0xff1d | 0xff1f | 0xff21 => {
                    // the cluster of a user-data set: not a <Cluster> record. A stored
                    // length of 0 means there is none.
                    let mut len = 0i32;
                    self.r.i32(&mut len);
                    if len > 0 {
                        let mut c = Cluster::default();
                        self.r.bytes(&mut [0u8; 1]); // an empty name
                        self.r.i32(&mut c.count);
                        let per = if t as u16 == 0xff1d && n.kind == 2 { 2 } else { 1 };
                        for _ in 0..self.count(c.count as i64 * per) {
                            if self.ended() {
                                break;
                            }
                            let mut v = 0i32;
                            self.r.i32(&mut v);
                            c.indices.push(v);
                        }
                        c.state = 0;
                        self.r.i32(&mut c.state);
                        n.ud_clusters[(t as u16 as usize - 0xff1d) / 2] = Some(c);
                    }
                }
                0xff1e | 0xff20 | 0xff22 => {
                    let mut len = 0i32;
                    self.r.i32(&mut len);
                    if len > 0 {
                        let mut count = 0i32;
                        self.r.i32(&mut count);
                        let mut arrays = None;
                        if count > 0 {
                            let mut a = Vec::new();
                            for _ in 0..self.count(count as i64) {
                                if self.ended() {
                                    break;
                                }
                                a.push(self.user_data());
                            }
                            arrays = Some(a);
                        }
                        n.ud_arrays[(t as u16 as usize - 0xff1e) / 2] = arrays;
                    }
                }
                0xff24 => {
                    // per UV set beyond the first: a count and that many ids. The number of
                    // sets comes from the mesh (its token 0x413), read earlier.
                    self.r.i32(&mut 0);
                    if n.uv_sets > 1 {
                        let mut all = Vec::new();
                        for _ in 0..self.count(n.uv_sets as i64 - 1) {
                            if self.ended() {
                                break;
                            }
                            let mut count = 0i32;
                            self.r.i32(&mut count);
                            let mut ids = Vec::new();
                            for _ in 0..self.count(count as i64) {
                                if self.ended() {
                                    break;
                                }
                                let mut v = 0i32;
                                self.r.i32(&mut v);
                                ids.push(v);
                            }
                            all.push(ids);
                        }
                        n.uv_domains = Some(all);
                    }
                }
                _ => self.skip(proto::HIERARCHY, t),
            }
            t = self.tok(proto::HIERARCHY, &mut i);
        }
        // a polygon naming a material the node does not have falls back to the first
        if n.kind == 4 {
            let nb = n.nb_materials;
            let mut reset = 0;
            if let Some(Definition::Mesh(m)) = n.definitions.first_mut() {
                for p in m.polygons.iter_mut() {
                    if p.material < 0 || nb <= p.material {
                        reset += (p.material != 0) as u32;
                        p.material = 0;
                    }
                }
            }
            if reset > 0 {
                self.changed("polygon material index out of range set to 0");
            }
        }
        n
    }

    /// The old-style user nodes of token 0x16: a count, then per node a 4-byte tag, the
    /// size and the data (part of [soft 0x083eca24 modelBinaryReadNode]).
    ///
    /// In a file below 3.50009 a tag other than `CUSA` to `CUSD` is byte-swapped and given a
    /// leading underscore, and the data is read as one item (reversed when 2, 4 or 8 bytes
    /// long). For the four custom-effect tags the original then converts the data's fields
    /// to the machine's byte order; here the data stays in file order.
    fn user_nodes(&mut self, n: &mut Node) {
        let mut count = 0i16;
        self.r.i16(&mut count);
        let old = below(self.versions.binary, 3.50009);
        for _ in 0..self.count(count as i64) {
            if self.ended() {
                break;
            }
            let mut tag = [0u8; 4];
            self.r.bytes(&mut tag);
            let mut node = UserNode { tag: tag.to_vec(), flag: 1, size: 0, data: Vec::new() };
            if old && !is_custom_tag(&tag) {
                tag.reverse();
                let s = crate::io::c_str(&tag);
                node.tag = [b"_", s].concat();
                self.changed("user-node tag swapped and prefixed (below 3.50009)");
            }
            self.r.i32(&mut node.size);
            node.data = self.data_block(node.size, old);
            n.user_nodes.push(node);
        }
    }

    /// One cluster of a node [soft 0x083f0be4 clustersReadBinaryChunk]. Token 1 (the name)
    /// creates the cluster at the end of the node's list (`SI_klusterAddToList`, 0x0843ab28,
    /// which also registers it with the editor; that is not done here); the other tokens
    /// fill the one created. On a face the index list holds two values per entry.
    ///
    /// A list whose first token is not the name makes the original use a pointer it never
    /// set; such tokens are read here and their values dropped.
    fn cluster(&mut self, n: &mut Node) {
        let mut cur: Option<usize> = None;
        let mut i = 0usize;
        let mut t = self.tok(proto::CLUSTER, &mut i);
        while t != 0 {
            match t {
                1 => {
                    let name = self.r.string();
                    n.clusters.push(Cluster { name, ..Cluster::default() });
                    cur = Some(n.clusters.len() - 1);
                }
                2 => {
                    let mut v = 0i32;
                    self.r.i32(&mut v);
                    if let Some(c) = cur {
                        n.clusters[c].count = v;
                    }
                }
                3 => {
                    if let Some(c) = cur {
                        let per = if n.kind == 2 { 2 } else { 1 };
                        let want = n.clusters[c].count as i64 * per;
                        let mut list = Vec::new();
                        for _ in 0..self.count(want) {
                            if self.ended() {
                                break;
                            }
                            let mut v = 0i32;
                            self.r.i32(&mut v);
                            list.push(v);
                        }
                        n.clusters[c].indices = list;
                    }
                }
                4 => {
                    let mut v = 0i32;
                    self.r.i32(&mut v);
                    if let Some(c) = cur {
                        n.clusters[c].state = v;
                    }
                }
                _ => self.skip(proto::CLUSTER, t),
            }
            t = self.tok(proto::CLUSTER, &mut i);
        }
    }

    /// [soft 0x083f1674 dynamatterReadBinaryChunk]
    fn dyna_matter(&mut self) -> DynaMatter {
        let mut d = DynaMatter::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::DYNA_MATTER, &mut i);
        while t != 0 {
            match t {
                1 => ok(self.r.f32(&mut d.density)),
                2 => ok(self.r.f32(&mut d.roughness)),
                3 => ok(self.r.f32(&mut d.elasticity)),
                4 => ok(self.r.f32(&mut d.rigidity)),
                5 => ok(self.r.f32(&mut d.plasticity)),
                6 => ok(self.r.f32(&mut d.kinetic)),
                7 => ok(self.r.i32(&mut d.collision)),
                _ => self.skip(proto::DYNA_MATTER, t),
            }
            t = self.tok(proto::DYNA_MATTER, &mut i);
        }
        d
    }

    /// The joint's friction [soft 0x083f0b08 dynajntpropReadBinaryChunk]
    /// [soft 0x0849fa5c SI_dynajntpropAllocate] [soft 0x0849f238 SI_dynajntpropSetDefault].
    fn joint_properties(&mut self) -> Vec3 {
        let mut f = [60.0, 15.0, 15.0];
        let mut i = 0usize;
        let mut t = self.tok(proto::JOINT, &mut i);
        while t != 0 {
            match t {
                1 => ok(self.r.floats(&mut f)),
                _ => self.skip(proto::JOINT, t),
            }
            t = self.tok(proto::JOINT, &mut i);
        }
        f
    }

    /// The dynamics chunk of hierarchy token 0xd [soft 0x084994b0 SI_odynamicReadBinaryChunk]:
    /// a flag and, if set, the joint's friction. Chain dynamics are allocated when the flag
    /// is clear or the node's algorithm is 12.
    fn old_dynamics(&mut self, n: &mut Node) {
        let mut flag = 0i16;
        self.r.i16(&mut flag);
        if flag != 0 {
            let mut f = [60.0, 15.0, 15.0];
            self.r.floats(&mut f);
            n.joint_friction = Some(f);
            if n.algorithm != 0xc {
                return;
            }
        }
        n.chain_dynamics = true;
    }

    /// [soft 0x084adb78 SI_invKinReadBinaryChunk]: type, number of joints, a rotation.
    ///
    /// With a positive joint count the original calls its root allocator
    /// (`SI_invKinRootAllocate`, 0x084ad9d4), which stores the count and replaces a type
    /// other than 40 by 10, 20 or 30 for one, two or more joints; that much of it is
    /// reproduced, its solver matrices are not.
    fn inv_kin(&mut self) -> InvKin {
        let mut k = InvKin::default();
        let mut joints = 0i32;
        self.r.i32(&mut k.kind);
        self.r.i32(&mut joints);
        self.r.floats(&mut k.rotation);
        let stored = k.kind;
        if joints > 0 {
            k.nb_joints = joints;
            if k.kind != 0x28 {
                k.kind = match joints {
                    1 => 10,
                    2 => 0x14,
                    _ => 0x1e,
                };
            }
        }
        if k.kind != stored || k.nb_joints != joints {
            self.changed("inverse kinematics: type or joint count replaced");
        }
        k
    }

    /// [soft 0x083f0d70 envelopVertReadBinaryChunk]: tokens 1 index, 3 ratio, 4 thickness,
    /// 6 weight; 2 (a vector) is read and dropped; 5 is a matrix that goes to `matrix`.
    fn envelop_vertex(&mut self, matrix: &mut [f32; 16]) -> EnvelopVertex {
        let mut v = EnvelopVertex::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::ENVELOP_VERTEX, &mut i);
        while t != 0 {
            match t {
                1 => ok(self.r.i32(&mut v.index)),
                2 => ok(self.r.floats(&mut [0f32; 3])),
                3 => ok(self.r.f32(&mut v.ratio)),
                4 => ok(self.r.f32(&mut v.thickness)),
                5 => self.r.matrix(matrix),
                6 => ok(self.r.f32(&mut v.weight)),
                _ => self.skip(proto::ENVELOP_VERTEX, t),
            }
            t = self.tok(proto::ENVELOP_VERTEX, &mut i);
        }
        v
    }

    fn envelop_list(&mut self, count: i32, old: bool, e: &mut Envelop) {
        let mut list = Vec::new();
        let mut matrices = Vec::new();
        for _ in 0..self.count(count as i64) {
            if self.ended() {
                break;
            }
            let mut m = [0f32; 16];
            list.push(self.envelop_vertex(&mut m));
            if old {
                matrices.push(m);
            }
        }
        e.lists.push(list);
        if old {
            e.old_matrices.push(matrices);
        }
    }

    /// The envelope of a node [soft 0x083f0e98 envelopReadBinaryChunk].
    ///
    /// Before anything is read the node gets an initial transformation, copied from its
    /// own, if it has none. Tokens 2 and 0x16 to 0x18 are vectors that belong to initial
    /// transformations rather than to the envelope: 2 is the node's rotation; 0x16, 0x17
    /// and 0x18 are scaling, rotation and translation of the *parent* when that is a chain
    /// root (a null with algorithm 1 or 2), and are dropped otherwise.
    ///
    /// The vertex lists come either as one list (token 10 with a 4-byte count, 0x13 with a
    /// short one) or as token 0x14, the number of lists, followed by token 0x15, each list
    /// with a 4-byte count. Up to version 2.59 each vertex also carried a matrix (its token
    /// 5); those are kept aside in `old_matrices`.
    fn envelop(&mut self, n: &mut Node, mut parent: Option<&mut Node>) -> Envelop {
        let mut e = Envelop::default();
        if n.init_transfo.is_none() {
            n.init_transfo = Some([n.scaling, n.rotation, n.translation]);
        }
        // not above the double 2.59 (0x0865bd90), the float version widened
        let old = (self.versions.binary as f64) <= 2.59;
        let mut lists = 0i16;
        let mut i = 0usize;
        let mut t = self.tok(proto::ENVELOP, &mut i);
        while t != 0 {
            match t {
                1 => ok(self.r.i32(&mut e.level)),
                2 => {
                    if let Some(it) = &mut n.init_transfo {
                        self.r.floats(&mut it[1]);
                    }
                }
                3 => ok(self.r.floats(&mut [0f32; 3])),
                4 => ok(self.r.i32(&mut e.in_type)),
                5 => ok(self.r.f32(&mut e.joint_area)),
                6 => ok(self.r.f32(&mut 0.0)),
                7..=9 => ok(self.r.f32(&mut e.inner[t as usize - 7])),
                0xb => ok(self.r.f32(&mut e.inner[3])),
                0xc => ok(self.r.i32(&mut e.out_type)),
                0xd..=0x10 => ok(self.r.f32(&mut e.outer[t as usize - 0xd])),
                10 | 0x13 => {
                    let mut count = 0i32;
                    if t == 10 {
                        self.r.i32(&mut count);
                    } else {
                        let mut c = 0i16;
                        self.r.i16(&mut c);
                        count = c as i32;
                    }
                    e.lists.clear(); // a new single list replaces what was there
                    e.old_matrices.clear();
                    self.envelop_list(count, old, &mut e);
                }
                0x11 | 0x12 => {
                    let mut count = 0i16;
                    self.r.i16(&mut count);
                    if count > 0 {
                        let mut curve = Vec::new();
                        for _ in 0..self.count(count as i64) {
                            if self.ended() {
                                break;
                            }
                            let mut v = 0f32;
                            self.r.f32(&mut v);
                            curve.push(v);
                        }
                        if curve.len() != 200 {
                            self.changed("deformation curve not of 200 values (always written as 200)");
                        }
                        if t == 0x11 {
                            e.in_curve = Some(curve);
                        } else {
                            e.out_curve = Some(curve);
                        }
                    }
                }
                0x14 => ok(self.r.i16(&mut lists)),
                0x15 => {
                    for _ in 0..self.count(lists as i64) {
                        if self.ended() {
                            break;
                        }
                        let mut count = 0i32;
                        self.r.i32(&mut count);
                        self.envelop_list(count, old, &mut e);
                    }
                }
                0x16..=0x18 => {
                    let mut dropped = [0f32; 3];
                    let target = match parent.as_deref_mut() {
                        Some(p) if p.kind == 0 && (p.algorithm.wrapping_sub(1) as u32) < 2 => {
                            &mut p.init_transfo.get_or_insert([[0.0; 3]; 3])[t as usize - 0x16]
                        }
                        _ => &mut dropped,
                    };
                    self.r.floats(target);
                }
                _ => self.skip(proto::ENVELOP, t),
            }
            t = self.tok(proto::ENVELOP, &mut i);
        }
        e
    }

    /// The envelope chunk of hierarchy token 0xf, which has no tokens
    /// [soft 0x084bbe28 SI_oEnvelopReadBinaryChunk] [soft 0x084bbd7c oEnvelopVertReadBinaryChunk]:
    /// level, two vectors (the second dropped), the inner curve's type, the joint area, a
    /// dropped float, three more floats, then one list of vertices: a count and per vertex
    /// index, a dropped vector, ratio and thickness.
    fn old_envelop(&mut self) -> Envelop {
        let mut e = Envelop::default();
        self.r.i32(&mut e.level);
        self.r.floats(&mut e.x54);
        self.r.floats(&mut [0f32; 3]);
        self.r.i32(&mut e.in_type);
        self.r.f32(&mut e.joint_area);
        self.r.f32(&mut 0.0);
        for k in 0..3 {
            self.r.f32(&mut e.inner[k]);
        }
        let mut count = 0i32;
        self.r.i32(&mut count);
        let mut list = Vec::new();
        for _ in 0..self.count(count as i64) {
            if self.ended() {
                break;
            }
            let mut v = EnvelopVertex::default();
            self.r.i32(&mut v.index);
            self.r.floats(&mut [0f32; 3]);
            self.r.f32(&mut v.ratio);
            self.r.f32(&mut v.thickness);
            list.push(v);
        }
        e.lists.push(list);
        e
    }

    /// A polygon mesh [soft 0x083ef514 meshBinaryRead]. Each list has an old token with a
    /// 16-bit count and a current one with a 32-bit count: vertices 3 and 7, polygons 4 and
    /// 8, edges 5 and 9. Token 0x413, the number of UV sets, is stored in the *node*.
    ///
    /// Not reproduced: after the read the original rebuilds the edges and, if no node
    /// normal was stored, the normals (`SI_meshUpdateNodes` and the three
    /// `SI_meshCompute…` routines).
    pub fn mesh(&mut self, n: &mut Node) -> Mesh {
        let mut m = Mesh::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::MESH, &mut i);
        let mut tmp = 0i16;
        while t != 0 {
            match t as u16 {
                1 => {
                    self.r.i16(&mut tmp);
                    if tmp != 0 {
                        m.flag |= 2;
                    }
                }
                2 => ok(self.r.f32(&mut m.angle)),
                3 | 7 => {
                    if t == 3 {
                        self.r.i16(&mut tmp);
                        m.nvertices = tmp as i32;
                    } else {
                        self.r.i32(&mut m.nvertices);
                    }
                    m.vertices = Vec::new();
                    for _ in 0..self.count(m.nvertices as i64) {
                        if self.ended() {
                            break;
                        }
                        let mut v = Vertex::default();
                        let mut j = 0usize;
                        let mut u = self.tok(proto::VERTEX, &mut j);
                        while u != 0 {
                            match u {
                                1 => ok(self.r.floats(&mut v.pos)),
                                4 => ok(self.r.i16(&mut v.flag)),
                                _ => self.skip(proto::VERTEX, u),
                            }
                            u = self.tok(proto::VERTEX, &mut j);
                        }
                        m.vertices.push(v);
                    }
                }
                4 | 8 => {
                    if t == 4 {
                        self.r.i16(&mut tmp);
                        m.npolygons = tmp as i32;
                    } else {
                        self.r.i32(&mut m.npolygons);
                    }
                    m.polygons = Vec::new();
                    for _ in 0..self.count(m.npolygons as i64) {
                        if self.ended() {
                            break;
                        }
                        let p = self.polygon(n, &mut m.compute_normals, &mut tmp);
                        m.polygons.push(p);
                    }
                }
                5 | 9 => {
                    if t == 5 {
                        self.r.i16(&mut tmp);
                        m.nedges = tmp as i32;
                    } else {
                        self.r.i32(&mut m.nedges);
                    }
                    m.edges = Vec::new();
                    for _ in 0..self.count(m.nedges as i64) {
                        if self.ended() {
                            break;
                        }
                        let mut e = Edge::default();
                        let mut j = 0usize;
                        let mut u = self.tok(proto::EDGE, &mut j);
                        while u != 0 {
                            match u {
                                1 => {
                                    for k in 0..2 {
                                        self.r.i16(&mut tmp);
                                        e.v[k] = tmp as i32;
                                    }
                                }
                                2 | 5 => {} // known, and nothing is stored for them
                                3 => ok(self.r.i16(&mut e.flag)),
                                4 => {
                                    self.r.i32(&mut e.v[0]);
                                    self.r.i32(&mut e.v[1]);
                                }
                                _ => self.skip(proto::EDGE, u),
                            }
                            u = self.tok(proto::EDGE, &mut j);
                        }
                        m.edges.push(e);
                    }
                }
                6 => ok(self.r.i16(&mut m.flag)),
                0x20a => ok(self.r.i16(&mut m.x52)),
                0x20b => ok(self.r.i16(&mut m.x50)),
                0x20c => ok(self.r.i16(&mut m.x38)),
                0x40d..=0x40f => ok(self.r.f32(&mut m.approx[t as usize - 0x40d])),
                0x210 => ok(self.r.i16(&mut m.x48)),
                0x211 => ok(self.r.i16(&mut m.x4a)),
                0x212 => ok(self.r.i16(&mut m.x4c)),
                0x413 => ok(self.r.i32(&mut n.uv_sets)),
                _ => self.skip(proto::MESH, t),
            }
            t = self.tok(proto::MESH, &mut i);
        }
        if m.compute_normals && m.polygons.iter().any(|p| !p.nodes.is_empty()) {
            self.changed("mesh without stored normals (the original computes them; not ported)");
        }
        m
    }

    /// One polygon and its nodes (part of [soft 0x083ef514 meshBinaryRead]).
    ///
    /// Node tokens: 1 the vertex index as a short, 3 as a 4-byte value; 2 the normal; 4 the
    /// texture coordinates; 0x405 the colour, whose first byte is forced to 0xff in a file
    /// below 3.71; 0xff06 the coordinates in the node's further UV sets.
    fn polygon(&mut self, n: &Node, compute_normals: &mut bool, tmp: &mut i16) -> Polygon {
        let mut p = Polygon::default();
        let mut j = 0usize;
        let mut u = self.tok(proto::POLYGON, &mut j);
        while u != 0 {
            match u {
                1 => ok(self.r.floats(&mut p.normal)),
                2 => {
                    self.r.i16(tmp);
                    p.nnodes = *tmp as u16;
                    p.nodes = Vec::new();
                    for _ in 0..self.count(p.nnodes as i64) {
                        if self.ended() {
                            break;
                        }
                        let mut d = PolyNode::default();
                        let mut k = 0usize;
                        let mut w = self.tok(proto::POLY_NODE, &mut k);
                        while w != 0 {
                            match w as u16 {
                                1 => {
                                    self.r.i16(tmp);
                                    d.vertex = *tmp as i32;
                                }
                                2 => {
                                    *compute_normals = false;
                                    self.r.floats(&mut d.normal);
                                }
                                3 => ok(self.r.i32(&mut d.vertex)),
                                4 => {
                                    self.r.f32(&mut d.uv[0]);
                                    self.r.f32(&mut d.uv[1]);
                                }
                                0x405 => {
                                    self.r.u32(&mut d.colour);
                                    // the constant is a float (0x0865bcc0)
                                    if self.versions.binary < 3.71f32 {
                                        if d.colour >> 24 != 0xff {
                                            self.changed("node colour: first byte forced to 0xff (below 3.71)");
                                        }
                                        d.colour |= 0xff00_0000;
                                    }
                                }
                                0xff06 => {
                                    self.r.i32(&mut 0);
                                    // (sets − 1) pairs; the original's allocation fails for
                                    // a negative count and it then writes through null
                                    let sets = n.uv_sets as i64 - 1;
                                    let mut more = Vec::new();
                                    for _ in 0..self.count(sets) {
                                        if self.ended() {
                                            break;
                                        }
                                        let mut uv = [0f32; 2];
                                        self.r.f32(&mut uv[0]);
                                        self.r.f32(&mut uv[1]);
                                        more.push(uv);
                                    }
                                    d.more_uv = more;
                                }
                                _ => self.skip(proto::POLY_NODE, w),
                            }
                            w = self.tok(proto::POLY_NODE, &mut k);
                        }
                        p.nodes.push(d);
                    }
                }
                3 => ok(self.r.i16(&mut p.material)),
                4 => ok(self.r.i16(&mut p.flag)),
                _ => self.skip(proto::POLYGON, u),
            }
            u = self.tok(proto::POLYGON, &mut j);
        }
        p
    }

    /// [soft 0x083f00dc splineBinaryRead]. Token 2 is the old key list, positions only;
    /// token 7 the current one, each key a record of the spline-key prototype.
    ///
    /// Not reproduced: the conversion of an old Bézier point list (`SI_bezierShrink`), the
    /// spline's matrix and the handling of phantom keys (`SI_allDummy`).
    pub fn spline(&mut self) -> Spline {
        let mut s = Spline::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::SPLINE, &mut i);
        while t != 0 {
            match t {
                1 => {
                    let mut k = 0i16;
                    self.r.i16(&mut k);
                    s.kind = k as i32;
                }
                2 => {
                    s.old_points = true;
                    self.r.u16(&mut s.nkeys);
                    s.keys = Vec::new();
                    for _ in 0..self.count(s.nkeys as i64) {
                        if self.ended() {
                            break;
                        }
                        let mut k = SplineKey::default();
                        self.r.floats(&mut k.pos);
                        s.keys.push(k);
                    }
                }
                3 => ok(self.r.i16(&mut s.close)),
                4 => ok(self.r.f32(&mut s.tension)),
                5 => ok(self.r.i16(&mut s.step)),
                6 => ok(self.r.i16(&mut s.phantom)),
                7 => {
                    self.r.u16(&mut s.nkeys);
                    s.keys = Vec::new();
                    for _ in 0..self.count(s.nkeys as i64) {
                        if self.ended() {
                            break;
                        }
                        let mut k = SplineKey::default();
                        let mut j = 0usize;
                        let mut u = self.tok(proto::SPLINE_KEY, &mut j);
                        while u != 0 {
                            match u {
                                1 => ok(self.r.floats(&mut k.prev)),
                                2 => ok(self.r.floats(&mut k.pos)),
                                3 => ok(self.r.floats(&mut k.next)),
                                4 => ok(self.r.i16(&mut k.constraint)),
                                5 => ok(self.r.i16(&mut k.segment)),
                                6 => ok(self.r.i16(&mut k.flag)),
                                _ => self.skip(proto::SPLINE_KEY, u),
                            }
                            u = self.tok(proto::SPLINE_KEY, &mut j);
                        }
                        s.keys.push(k);
                    }
                }
                8 => s.name = self.r.string(),
                _ => self.skip(proto::SPLINE, t),
            }
            t = self.tok(proto::SPLINE, &mut i);
        }
        s
    }

    /// A spline of a face: read as any spline, then copied into the face's own record by
    /// `SI_splineTransfer` (0x0848aa9c), which copies no name: the outline and the holes of
    /// a face have none, whatever token 8 stored (finding 0004, second image pass, point 6).
    fn face_spline(&mut self) -> Spline {
        let mut s = self.spline();
        if !s.name.is_empty() {
            self.changed("face spline: name dropped (SI_splineTransfer copies none)");
        }
        s.name.clear();
        s
    }

    /// [soft 0x083f043c faceBinaryRead]. Token 3 says whether the face is made of splines
    /// (the default) or of NURBS curves; tokens 1 and 2 (outline, holes) are read only in
    /// the first case and token 4 (the curves) only in the second, and a token that does
    /// not apply reads nothing.
    pub fn face(&mut self) -> Face {
        let mut f = Face::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::FACE, &mut i);
        while t != 0 {
            match t {
                1 => {
                    if f.is_spline != 0 {
                        f.exterior = self.face_spline();
                    }
                }
                2 => {
                    if f.is_spline != 0 {
                        self.r.i16(&mut f.count);
                        f.holes = Vec::new();
                        for _ in 0..self.count(f.count as i64) {
                            if self.ended() {
                                break;
                            }
                            let s = self.face_spline();
                            f.holes.push(s);
                        }
                    }
                }
                3 => ok(self.r.i16(&mut f.is_spline)),
                4 => {
                    if f.is_spline == 0 {
                        self.r.i16(&mut f.count);
                        f.nurbs = Vec::new();
                        for _ in 0..self.count(f.count as i64) {
                            if self.ended() {
                                break;
                            }
                            f.nurbs.push(self.nurbs_curve());
                        }
                    }
                }
                _ => self.skip(proto::FACE, t),
            }
            t = self.tok(proto::FACE, &mut i);
        }
        f
    }

    /// [soft 0x083f0690 patchBinaryRead]. An unknown token is skipped as a long one
    /// whatever its length byte says.
    pub fn patch(&mut self) -> Patch {
        let mut p = Patch::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::PATCH, &mut i);
        let mut tmp = 0i16;
        while t != 0 {
            let r = &mut self.r;
            match t {
                1 | 2 => {
                    r.i16(&mut tmp);
                    p.kind[t as usize - 1] = tmp as i32;
                }
                3 => {
                    r.u16(&mut p.nu);
                    r.u16(&mut p.nv);
                    let total = p.nu as i64 * p.nv as i64;
                    p.points = Vec::new();
                    for _ in 0..self.count(total) {
                        if self.ended() {
                            break;
                        }
                        let mut v = [0f32; 3];
                        self.r.floats(&mut v);
                        p.points.push(v);
                    }
                    p.tags = vec![0; p.points.len()];
                }
                4 | 5 => {
                    let c = &mut p.close[t as usize - 4];
                    r.i16(c);
                    if *c != 0 && *c != 1 {
                        *c = 1;
                        self.changed("patch close flag set to 1");
                    }
                }
                6 | 7 => ok(r.f32(&mut p.tension[t as usize - 6])),
                8 | 9 => ok(r.i16(&mut p.step[t as usize - 8])),
                10 | 0xb => ok(r.i16(&mut p.curves[t as usize - 10])),
                0xc..=0xf => ok(r.i16(&mut p.caps[t as usize - 0xc])),
                0x10 => ok(r.i16(&mut p.faceted)),
                0x11 => ok(r.i16(&mut p.approx)),
                0x12 => ok(r.i16(&mut p.view)),
                0x13 => ok(r.f32(&mut p.edge)),
                0x14 | 0x15 => ok(r.f32(&mut p.curvature[t as usize - 0x14])),
                0x16 => ok(r.i16(&mut p.rec_min)),
                0x17 => ok(r.i16(&mut p.rec_max)),
                0x18 => ok(r.i16(&mut p.recursion)),
                0x19 => {
                    for tag in p.tags.iter_mut() {
                        r.i16(tag);
                    }
                }
                _ => self.skip_as_long(proto::PATCH, t),
            }
            t = self.tok(proto::PATCH, &mut i);
        }
        p
    }

    /// [soft 0x083f257c metaSystemBinaryRead] [soft 0x0845dd04 SI_metaSystemAddGroup].
    /// Groups, the primitives of a group and unblend pairs are each linked in at the head
    /// of their list, so every list ends up in the reverse of the file's order.
    pub fn metasystem(&mut self) -> MetaSystem {
        let mut m = MetaSystem::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::METASYSTEM, &mut i);
        let mut count = 0i16;
        while t != 0 {
            match t {
                1 => ok(self.r.f32(&mut m.wire_res)),
                2 => ok(self.r.f32(&mut m.render_res)),
                3 => ok(self.r.i16(&mut m.show)),
                4 => {
                    self.r.i16(&mut count);
                    for _ in 0..self.count(count as i64) {
                        if self.ended() {
                            break;
                        }
                        let mut cur: Option<usize> = None;
                        let mut j = 0usize;
                        let mut u = self.tok(proto::METAGROUP, &mut j);
                        while u != 0 {
                            match u {
                                1 => {
                                    let name = self.r.string();
                                    cur = Some(match m.groups.iter().position(|g| g.name == name) {
                                        Some(at) => at,
                                        None => {
                                            m.groups.insert(0, MetaGroup { name, primitives: Vec::new() });
                                            0
                                        }
                                    });
                                }
                                2 => {
                                    let mut k = 0i16;
                                    self.r.i16(&mut k);
                                    for _ in 0..self.count(k as i64) {
                                        if self.ended() {
                                            break;
                                        }
                                        let name = self.r.string();
                                        if let Some(g) = cur {
                                            m.groups[g].primitives.insert(0, name);
                                        }
                                    }
                                }
                                _ => self.skip(proto::METAGROUP, u),
                            }
                            u = self.tok(proto::METAGROUP, &mut j);
                        }
                    }
                }
                5 => {
                    self.r.i16(&mut count);
                    for _ in 0..self.count(count as i64) {
                        if self.ended() {
                            break;
                        }
                        m.unblends.insert(0, [Vec::new(), Vec::new()]);
                        let mut j = 0usize;
                        let mut u = self.tok(proto::METAUNBLEND, &mut j);
                        while u != 0 {
                            match u {
                                1 => {
                                    let a = self.r.string();
                                    let b = self.r.string();
                                    m.unblends[0] = [a, b];
                                }
                                _ => self.skip(proto::METAUNBLEND, u),
                            }
                            u = self.tok(proto::METAUNBLEND, &mut j);
                        }
                    }
                }
                _ => self.skip(proto::METASYSTEM, t),
            }
            t = self.tok(proto::METASYSTEM, &mut i);
        }
        m
    }

    /// [soft 0x083f23ec metaballBinaryRead]. Tokens 1 to 3 are 4-byte values that are read
    /// and dropped.
    pub fn metaball(&mut self) -> Metaball {
        let mut b = Metaball::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::METABALL, &mut i);
        while t != 0 {
            match t {
                1..=3 => ok(self.r.i32(&mut 0)),
                4 => ok(self.r.f64(&mut b.ratio)),
                5 => ok(self.r.f64(&mut b.weight)),
                6 => self.r.doubles3(&mut b.exponent),
                7 => self.r.matrix(&mut b.matrix),
                _ => self.skip(proto::METABALL, t),
            }
            t = self.tok(proto::METABALL, &mut i);
        }
        b
    }

    fn nurbs_points(&mut self, total: i64) -> Vec<NurbsPoint> {
        // a point's flag is stored when the version is not below the double 2.6
        // (0x0865bf30): the float 2.6 (2.5999999) has none
        let with_flag = (self.versions.binary as f64) >= 2.6;
        let mut points = Vec::new();
        for _ in 0..self.count(total) {
            if self.ended() {
                break;
            }
            let mut p = NurbsPoint::default();
            for c in p.xyzw.iter_mut() {
                self.r.f64(c);
            }
            if with_flag {
                self.r.i32(&mut p.flag);
            }
            points.push(p);
        }
        points
    }

    fn knots(&mut self, count: i32) -> Vec<f64> {
        let mut knots = Vec::new();
        for _ in 0..self.count(count as i64) {
            if self.ended() {
                break;
            }
            let mut k = 0f64;
            self.r.f64(&mut k);
            knots.push(k);
        }
        knots
    }

    /// [soft 0x083f2b38 nurbsCrvBinaryRead] [soft 0x084dc500 si_knotsAllocate]. Tokens 8 and
    /// 0x208 are the same field.
    pub fn nurbs_curve(&mut self) -> NurbsCurve {
        let mut c = NurbsCurve::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::NURBS_CURVE, &mut i);
        while t != 0 {
            match t {
                1 => ok(self.r.i32(&mut c.order)),
                2 => {
                    self.r.i32(&mut c.npoints);
                    c.points = self.nurbs_points(c.npoints as i64);
                }
                3 => ok(self.r.i16(&mut c.closed)),
                4 => {
                    self.r.i32(&mut c.uniformity);
                    self.r.i32(&mut c.nknots);
                    c.knots = self.knots(c.nknots);
                }
                5 => {
                    self.r.f64(&mut c.start);
                    self.r.f64(&mut c.end);
                }
                6 => ok(self.r.i32(&mut c.step)),
                7 => ok(self.r.i32(&mut c.closing)),
                8 | 0x208 => ok(self.r.i16(&mut c.keyframed)),
                _ => self.skip(proto::NURBS_CURVE, t),
            }
            t = self.tok(proto::NURBS_CURVE, &mut i);
        }
        c
    }

    fn composites(&mut self, count: i32) -> Vec<NurbsComposite> {
        let mut list = Vec::new();
        for _ in 0..self.count(count as i64) {
            if self.ended() {
                break;
            }
            list.push(self.nurbs_composite());
        }
        list
    }

    /// [soft 0x083f2d90 nurbsSrfBinaryRead]. Token 0xff1a (curves projected on the surface)
    /// is token 0x1a behind a length.
    ///
    /// Not reproduced: the capping the original forces on an open surface whose capping
    /// flags are set (`SI_nurbsSrfForceCapping`).
    pub fn nurbs_surface(&mut self) -> NurbsSurface {
        let mut s = NurbsSurface::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::NURBS_SURFACE, &mut i);
        while t != 0 {
            let r = &mut self.r;
            match t as u16 {
                1 => {
                    r.i32(&mut s.order[0]);
                    r.i32(&mut s.order[1]);
                }
                2 => {
                    r.i32(&mut s.count[0]);
                    r.i32(&mut s.count[1]);
                    let total = s.count[0].wrapping_mul(s.count[1]);
                    s.points = self.nurbs_points(total as i64);
                }
                3 => {
                    r.i16(&mut s.closed[0]);
                    r.i16(&mut s.closed[1]);
                }
                4 => {
                    r.i32(&mut s.uniformity);
                    for k in 0..2 {
                        self.r.i32(&mut s.nknots[k]);
                        s.knots[k] = self.knots(s.nknots[k]);
                    }
                }
                5 => {
                    for v in s.range.iter_mut() {
                        r.f64(v);
                    }
                }
                6 => ok(r.i16(&mut s.border)),
                7 => {
                    r.i32(&mut s.ntrims);
                    if s.ntrims > 0 {
                        s.trims = self.composites(s.ntrims);
                    }
                }
                8 | 9 => ok(r.i32(&mut s.steps[t as usize - 8])),
                10 | 0xb => ok(r.i32(&mut s.curves[t as usize - 10])),
                0xc..=0xf => ok(r.i16(&mut s.caps[t as usize - 0xc])),
                0x10 | 0x11 => ok(r.i32(&mut s.closing[t as usize - 0x10])),
                0x12 => ok(r.i16(&mut s.approx)),
                0x13 => ok(r.i16(&mut s.view)),
                0x14 => ok(r.f32(&mut s.edge)),
                0x15 | 0x16 => ok(r.f32(&mut s.curvature[t as usize - 0x15])),
                0x17 => ok(r.i16(&mut s.rec_min)),
                0x18 => ok(r.i16(&mut s.rec_max)),
                0x19 => ok(r.i16(&mut s.recursion)),
                0x1a | 0xff1a => {
                    if t as u16 == 0xff1a {
                        r.i32(&mut 0);
                    }
                    r.i32(&mut s.nprojected);
                    if s.nprojected > 0 {
                        s.projected = self.composites(s.nprojected);
                    }
                }
                _ => self.skip(proto::NURBS_SURFACE, t),
            }
            t = self.tok(proto::NURBS_SURFACE, &mut i);
        }
        s
    }

    /// A trimming or projected curve made of several curves
    /// [soft 0x083f32e4 nurbsCompBinaryRead]. Like the patch reader it skips every unknown
    /// token as a long one.
    pub fn nurbs_composite(&mut self) -> NurbsComposite {
        let mut c = NurbsComposite::default();
        let mut i = 0usize;
        let mut t = self.tok(proto::NURBS_COMPOSITE, &mut i);
        while t != 0 {
            match t {
                1 => ok(self.r.i16(&mut c.boundary)),
                2 => {
                    self.r.i32(&mut c.ncurves);
                    c.curves = Vec::new();
                    for _ in 0..self.count(c.ncurves as i64) {
                        if self.ended() {
                            break;
                        }
                        c.curves.push(self.nurbs_curve());
                    }
                }
                _ => self.skip_as_long(proto::NURBS_COMPOSITE, t),
            }
            t = self.tok(proto::NURBS_COMPOSITE, &mut i);
        }
        c
    }
}
