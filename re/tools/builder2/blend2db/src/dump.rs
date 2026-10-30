// SPDX-License-Identifier: MIT
//! `blend2db dump <model.hrc>`: the first nodes of a model file, for looking at what a mesh model holds.
use si_hrc::{Content, Definition, Kind};

pub fn run(path: &str) -> i32 {
    let data = std::fs::read(path).expect("file");
    if path.ends_with(".cam") {
        let mut cam = si_elemio::camera::Camera::perspective_default();
        let f = si_elemio::camera::read_file(&mut si_elemio::Reader::new(&data), &mut cam);
        println!("version {} {:#?}", f.version, cam);
        return 0;
    }
    if path.ends_with(".lig") && std::env::var("DUMP_ONELINE").is_ok() {
        let f = si_elemio::light::read_file(&mut si_elemio::Reader::new(&data), 0);
        if let Some(l) = f.read.light {
            let n: String = l.name.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
            println!("{}\tkind {}\tcol {:?}\tcone {:.4}\tspread {:.4}\tfall {} {}\tshadow {} fac {} res {}\tfile {}", n, l.kind, l.colour, l.cone_angle, l.spread_angle, l.fall_off_start, l.fall_off_end, l.shadow_type, l.shadow_factor, l.shadow_map_res, path.rsplit('/').next().unwrap_or(""));
        }
        return 0;
    }
    if path.ends_with(".lig") {
        let f = si_elemio::light::read_file(&mut si_elemio::Reader::new(&data), 0);
        println!("version {} {:#?}", f.version, f.read.light);
        return 0;
    }
    if path.ends_with(".shp") {
        // shape file (unit anim1): the kind from the tag, then the definitions' sizes and first vertices
        let mut sm = si_elemio::shape::ShapeModel::new(4);
        let mut f = si_elemio::shape::read_file(&mut si_elemio::Reader::new(&data), &mut sm);
        if !f.accepted {
            if let Some(t) = si_elemio::shape::model_type_of(f.tag) {
                sm = si_elemio::shape::ShapeModel::new(t);
                f = si_elemio::shape::read_file(&mut si_elemio::Reader::new(&data), &mut sm);
            }
        }
        println!("version {} tag {:?} current {} count {} weight_mode {} defs {} accepted {}", f.version, f.tag.to_be_bytes().map(|b| b as char), f.has_current, f.count, sm.weight_mode, sm.definitions.len(), f.accepted);
        for (i, g) in sm.current.iter().chain(sm.definitions.iter()).enumerate() {
            match g {
                si_elemio::geom::Geometry::Mesh(m) => println!("  def {i} mesh flag {:#x} nv {} np {} ne {} normals_read {} edge_field {} v0 {:?} p0 {:?}", m.flag, m.nb_vertices, m.nb_polygons, m.nb_edges, m.normals_read, m.edge_field, m.vertices.first(), m.polygons.first().map(|p| (p.nb_nodes, p.material, p.flag, p.nodes.first().map(|n| (n.vertex, n.normal, n.uv, n.colour))))),
                other => println!("  def {i} kind {}", other.model_type()),
            }
        }
        return 0;
    }
    if path.ends_with(".ani") {
        let f = si_elemio::anim::read_file(&mut si_elemio::Reader::new(&data), 1);
        println!("version {} chapter {:?}", f.version, f.anim.chapter.as_ref().map(|c| String::from_utf8_lossy(c).to_string()));
        for (i, e) in f.anim.elems.iter().enumerate() {
            println!("elem {i} {:?} father {}", String::from_utf8_lossy(&e.name), e.father);
            for it in &e.items {
                match &it.data {
                    si_elemio::anim::ItemData::Curve(c) => {
                        let k: Vec<String> = c.keys.iter().take(4).map(|k| format!("({:.4},{:.4} slope {:.3}/{:.3} fl {} int {} ty {})", k.time, k.value, k.left_slope, k.right_slope, k.flag, k.interpolation, k.key_type)).collect();
                        println!("  item kind {} relid {} occ {} w20 {} curve type {} keys {} pre {} post {} flags {} interp {} {}", it.kind, it.act_rel_id, it.act_occ_id, it.w_20, c.curve_type, c.count, c.pre_extrapolation, c.post_extrapolation, c.flags, c.interpolation, k.join(" "));
                    }
                    d => println!("  item kind {} {:?}", it.kind, d),
                }
            }
        }
        return 0;
    }
    if path.ends_with(".txt") || path.ends_with(".mtr") {
        let f = si_hrc::read_file(&data, if path.ends_with(".txt") { Kind::Texture } else { Kind::Material }).expect("read");
        println!("{:?}", f.content);
        return 0;
    }
    let f = si_hrc::read_file(&data, Kind::Model).expect("read");
    let Content::Model(root) = f.content else { return 1 };
    root.walk(0, &mut |d, n| {
        println!("{}node {:?} kind {} scal {:?} rot {:?} trans {:?} nbmat {} mats {} tex {} vis {} blur {} matte {} uvsets {} flag8 {} shape_interp {} alg {} envtype {} clusters {} usernodes {}",
            " ".repeat(d * 2), String::from_utf8_lossy(&n.name), n.kind, n.scaling, n.rotation, n.translation, n.nb_materials, n.materials.len(),
            n.textures.len(), n.visibility, n.motion_blur, n.matte, n.uv_sets, n.flag8, n.shape_interp, n.algorithm, n.envelop_type, n.clusters.len(), n.user_nodes.len());
        for m in n.materials.iter().flatten() {
            println!("{}  material {:?}", " ".repeat(d * 2), m);
        }
        for def in &n.definitions {
            match def {
                Definition::Mesh(m) => {
                    println!("{}  mesh flag {:#x} nv {} np {} ne {} angle {} compute_normals {} x {} {} {} {} {} {} {:?}", " ".repeat(d * 2), m.flag, m.nvertices, m.npolygons, m.nedges, m.angle, m.compute_normals,
                        m.x38, m.x48, m.x4a, m.x4c, m.x50, m.x52, m.approx);
                    for v in m.vertices.iter().take(3) { println!("{}   v {:?}", " ".repeat(d * 2), v); }
                    for p in m.polygons.iter().take(2) { println!("{}   poly {:?}", " ".repeat(d * 2), p); }
                    for e in m.edges.iter().take(3) { println!("{}   edge {:?}", " ".repeat(d * 2), e); }
                }
                other => println!("{}  definition {}", " ".repeat(d * 2), format!("{:?}", other).chars().take(40).collect::<String>()),
            }
        }
    });
    0
}
