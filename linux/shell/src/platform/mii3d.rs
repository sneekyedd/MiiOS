//! A tiny 3D renderer for your Miis (.glb files), drawn by the CPU.
//!
//! How it works, step by step:
//!  1. **Load** the .glb: a glTF 3D model = a tree of *nodes* (bones), *meshes*
//!     (triangles with positions, normals and texture coordinates), *materials*
//!     (colours and pictures) and a *skin* (which bones move which vertices).
//!  2. **Pose** the skeleton: every bone has a position/rotation relative to its
//!     parent. To make a Mii walk we add small rotations to the hip, knee and
//!     shoulder bones (a sine wave), and to wave we lift one arm.
//!  3. **Skin**: each body vertex follows up to 4 bones, mixed by weights
//!     ("linear blend skinning"). Head parts simply follow the head bone.
//!  4. **Rasterise**: project every triangle onto the picture, and for each
//!     pixel inside it keep the closest surface (a *depth buffer*), look up the
//!     texture colour, and shade it with a soft light from the front-top.
//!     See-through parts (the face's eyes and mouth) are blended on top.
//! The picture is drawn at twice the size and averaged down, for smooth edges.

use std::collections::HashMap;

type V3 = [f32; 3];
type Q = [f32; 4]; // quaternion x, y, z, w
type M4 = [f32; 16]; // column-major, like glTF

fn qmul(a: Q, b: Q) -> Q {
    [a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
     a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
     a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
     a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2]]
}
fn qaxis(axis: V3, angle: f32) -> Q { let (s, c) = (angle * 0.5).sin_cos(); [axis[0] * s, axis[1] * s, axis[2] * s, c] }
fn trs(t: V3, r: Q, s: V3) -> M4 {
    let [x, y, z, w] = r;
    let (xx, yy, zz, xy, xz, yz, wx, wy, wz) = (x * x, y * y, z * z, x * y, x * z, y * z, w * x, w * y, w * z);
    [(1.0 - 2.0 * (yy + zz)) * s[0], 2.0 * (xy + wz) * s[0], 2.0 * (xz - wy) * s[0], 0.0,
     2.0 * (xy - wz) * s[1], (1.0 - 2.0 * (xx + zz)) * s[1], 2.0 * (yz + wx) * s[1], 0.0,
     2.0 * (xz + wy) * s[2], 2.0 * (yz - wx) * s[2], (1.0 - 2.0 * (xx + yy)) * s[2], 0.0,
     t[0], t[1], t[2], 1.0]
}
fn mmul(a: &M4, b: &M4) -> M4 {
    let mut o = [0.0; 16];
    for c in 0..4 { for r in 0..4 { o[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum(); } }
    o
}
fn xpoint(m: &M4, p: V3) -> V3 {
    [m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12], m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13], m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14]]
}
fn xdir(m: &M4, p: V3) -> V3 { [m[0] * p[0] + m[4] * p[1] + m[8] * p[2], m[1] * p[0] + m[5] * p[1] + m[9] * p[2], m[2] * p[0] + m[6] * p[1] + m[10] * p[2]] }
fn norm(v: V3) -> V3 { let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6); [v[0] / l, v[1] / l, v[2] / l] }

struct Node { parent: Option<usize>, t: V3, r: Q, s: V3, name: String }
struct Prim { node: usize, skinned: bool, pos: Vec<V3>, nrm: Vec<V3>, uv: Vec<[f32; 2]>, joints: Vec<[u16; 4]>, weights: Vec<[f32; 4]>, idx: Vec<u32>, mat: usize }
struct Mat { color: [f32; 4], tex: Option<usize>, blend: bool, mirror_u: bool, mirror_v: bool }
struct Tex { w: usize, h: usize, px: Vec<[u8; 4]> }

pub struct Model {
    pub name: String,
    /// Who made this Mii (from the Mii Creator data), if known.
    pub creator: Option<String>,
    nodes: Vec<Node>,
    prims: Vec<Prim>,
    joints: Vec<usize>,
    ibm: Vec<M4>,
    mats: Vec<Mat>,
    texs: Vec<Tex>,
    bone: HashMap<String, usize>,
    /// For each node: the dials that turn it (axis, dial).
    dials: Vec<Vec<([f32; 3], usize)>>,
    min: V3,
    max: V3,
}

/// How to pose the Mii for one picture: the shared joint "dials"
/// (`crate::host::MiiPose`) plus a hook for testing single bones.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Pose {
    pub test: Option<(&'static str, usize, f32)>,
    pub mii: crate::host::MiiPose,
}

/// Which bone, around which of its own axes, with which sign, each dial turns.
/// Found by rendering every bone around every axis and looking (see README).
const DIALS: [(&str, [f32; 3], usize); 18] = {
    use crate::host::j::*;
    // Mii arm bones are turned about 45 degrees from the body, so "forward"
    // and "out to the side" are each a mix of two of the bone's own axes.
    const H: f32 = std::f32::consts::FRAC_1_SQRT_2;
    [
        ("Spine_2", [0.0, -1.0, 0.0], SPINE_BEND), ("Spine_2", [1.0, 0.0, 0.0], SPINE_TWIST), ("Spine_2", [0.0, 0.0, 1.0], SPINE_TILT),
        ("Head", [0.0, -1.0, 0.0], HEAD_NOD), ("Head", [1.0, 0.0, 0.0], HEAD_TURN), ("Head", [0.0, 0.0, 1.0], HEAD_TILT),
        ("Arm_1_L", [0.0, -H, H], ARM_L_SWING), ("Arm_1_L", [0.0, H, H], ARM_L_RAISE), ("Arm_2_L", [0.0, -H, H], ELBOW_L),
        ("Arm_1_R", [0.0, H, H], ARM_R_SWING), ("Arm_1_R", [0.0, -H, H], ARM_R_RAISE), ("Arm_2_R", [0.0, H, H], ELBOW_R),
        ("Leg_1_L", [0.0, 1.0, 0.0], LEG_L_SWING), ("Leg_2_L", [0.0, -1.0, 0.0], KNEE_L), ("Leg_1_R", [0.0, -1.0, 0.0], LEG_R_SWING), ("Leg_2_R", [0.0, 1.0, 0.0], KNEE_R),
        ("Leg_1_L", [0.0, 0.0, -1.0], LEG_L_OUT), ("Leg_1_R", [0.0, 0.0, -1.0], LEG_R_OUT),
    ]
};

fn decode_png(bytes: &[u8]) -> Option<Tex> {
    let mut d = png::Decoder::new(std::io::Cursor::new(bytes));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let ch = match info.color_type { png::ColorType::Rgba => 4, png::ColorType::Rgb => 3, png::ColorType::GrayscaleAlpha => 2, _ => 1 };
    let px = buf[..info.buffer_size()].chunks(ch).map(|p| match ch {
        4 => [p[0], p[1], p[2], p[3]], 3 => [p[0], p[1], p[2], 255], 2 => [p[0], p[0], p[0], p[1]], _ => [p[0], p[0], p[0], 255],
    }).collect();
    Some(Tex { w: info.width as usize, h: info.height as usize, px })
}

impl Model {
    pub fn load(bytes: &[u8]) -> Result<Model, String> {
        let g = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
        let blob: &[u8] = g.blob.as_deref().ok_or("no binary data in the .glb")?;
        let buf = |b: gltf::Buffer| if b.index() == 0 { Some(blob) } else { None };
        let doc = &g.document;

        // Name from the Mii data, else the file's root node.
        let extras = doc.as_json().asset.extras.as_ref().map(|e| e.get().to_string()).unwrap_or_default();
        let name = json_str(&extras, "name").unwrap_or_else(|| String::from("Mii"));
        let creator = json_str(&extras, "creator");

        let mut nodes: Vec<Node> = doc.nodes().map(|n| {
            let (t, r, s) = n.transform().decomposed();
            Node { parent: None, t, r, s, name: n.name().unwrap_or("").to_string() }
        }).collect();
        for n in doc.nodes() { for c in n.children() { nodes[c.index()].parent = Some(n.index()); } }

        let texs: Vec<Tex> = doc.images().map(|im| match im.source() {
            gltf::image::Source::View { view, .. } => {
                let s = view.offset();
                decode_png(&blob[s..s + view.length()]).unwrap_or(Tex { w: 1, h: 1, px: vec![[255; 4]] })
            }
            _ => Tex { w: 1, h: 1, px: vec![[255; 4]] },
        }).collect();
        let mats: Vec<Mat> = doc.materials().map(|m| {
            let p = m.pbr_metallic_roughness();
            let tex = p.base_color_texture();
            let mirror = |w: gltf::texture::WrappingMode| w == gltf::texture::WrappingMode::MirroredRepeat;
            let (mu, mv) = tex.as_ref().map(|t| { let s = t.texture().sampler(); (mirror(s.wrap_s()), mirror(s.wrap_t())) }).unwrap_or((false, false));
            Mat { color: p.base_color_factor(), tex: tex.map(|t| t.texture().source().index()),
                  blend: m.alpha_mode() == gltf::material::AlphaMode::Blend, mirror_u: mu, mirror_v: mv }
        }).collect();

        let mut prims = Vec::new();
        for n in doc.nodes() {
            let Some(mesh) = n.mesh() else { continue };
            for p in mesh.primitives() {
                let r = p.reader(buf);
                let pos: Vec<V3> = r.read_positions().map(|i| i.collect()).unwrap_or_default();
                let cnt = pos.len();
                let nrm = r.read_normals().map(|i| i.collect()).unwrap_or_else(|| vec![[0.0, 0.0, 1.0]; cnt]);
                let uv = r.read_tex_coords(0).map(|i| i.into_f32().collect()).unwrap_or_else(|| vec![[0.0; 2]; cnt]);
                let joints: Vec<[u16; 4]> = r.read_joints(0).map(|i| i.into_u16().collect()).unwrap_or_default();
                let weights: Vec<[f32; 4]> = r.read_weights(0).map(|i| i.into_f32().collect()).unwrap_or_default();
                let idx = r.read_indices().map(|i| i.into_u32().collect()).unwrap_or_else(|| (0..cnt as u32).collect());
                let skinned = n.skin().is_some() && joints.len() == cnt;
                prims.push(Prim { node: n.index(), skinned, pos, nrm, uv, joints, weights, idx, mat: p.material().index().unwrap_or(0) });
            }
        }
        let (mut joints, mut ibm) = (Vec::new(), Vec::new());
        if let Some(skin) = doc.skins().next() {
            joints = skin.joints().map(|j| j.index()).collect();
            ibm = skin.reader(buf).read_inverse_bind_matrices().map(|i| i.map(|m| {
                let mut o = [0.0; 16];
                for c in 0..4 { for r in 0..4 { o[c * 4 + r] = m[c][r]; } }
                o
            }).collect()).unwrap_or_else(|| vec![trs([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]); joints.len()]);
        }
        if mats.is_empty() { return Err("model has no materials".into()); }
        let bone: std::collections::HashMap<String, usize> = nodes.iter().enumerate().map(|(i, n)| (n.name.clone(), i)).collect();
        let mut dials = vec![Vec::new(); nodes.len()];
        for (bname, axis, dial) in DIALS { if let Some(&n) = bone.get(bname) { dials[n].push((axis, dial)); } }
        let mut m = Model { name, creator, nodes, prims, joints, ibm, mats, texs, bone, dials, min: [0.0; 3], max: [0.0; 3] };
        // Size of the Mii standing still (used to fit it into its picture).
        let (verts, _) = m.posed(&Pose::default());
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in verts.iter().flatten() { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
        m.min = lo; m.max = hi;
        Ok(m)
    }

    /// Height in the model's own units (for "taller Miis look taller").
    pub fn height(&self) -> f32 { self.max[1] - self.min[1] }

    /// Extra bone rotation for this pose: the product of every dial that turns this bone.
    fn bend(&self, pose: &Pose, node: usize) -> Option<Q> {
        if let Some((b, ax, ang)) = pose.test {
            if b == self.nodes[node].name { let mut a = [0.0; 3]; a[ax] = 1.0; return Some(qaxis(a, ang)); }
        }
        let mut q: Option<Q> = None;
        for &(axis, dial) in &self.dials[node] {
            let v = pose.mii.j[dial];
            if v == 0 { continue; }
            let r = qaxis(axis, v as f32 / 256.0 * std::f32::consts::TAU);
            q = Some(match q { Some(p) => qmul(p, r), None => r });
        }
        q
    }

    /// World matrix of every node for this pose.
    fn world(&self, pose: &Pose) -> Vec<M4> {
        let mut w: Vec<Option<M4>> = vec![None; self.nodes.len()];
        fn get(m: &Model, pose: &Pose, i: usize, w: &mut Vec<Option<M4>>) -> M4 {
            if let Some(x) = w[i] { return x; }
            let n = &m.nodes[i];
            let r = match m.bend(pose, i) { Some(q) => qmul(n.r, q), None => n.r };
            let local = trs(n.t, r, n.s);
            let x = match n.parent { Some(p) => { let pm = get(m, pose, p, w); mmul(&pm, &local) } None => local };
            w[i] = Some(x);
            x
        }
        (0..self.nodes.len()).map(|i| get(self, pose, i, &mut w)).collect()
    }

    /// Every primitive's vertices (and normals) in world space, posed.
    fn posed(&self, pose: &Pose) -> (Vec<Vec<V3>>, Vec<Vec<V3>>) {
        let w = self.world(pose);
        let jm: Vec<M4> = self.joints.iter().zip(&self.ibm).map(|(&j, ib)| mmul(&w[j], ib)).collect();
        let mut vs = Vec::with_capacity(self.prims.len());
        let mut ns = Vec::with_capacity(self.prims.len());
        for p in &self.prims {
            if p.skinned {
                let mut v = Vec::with_capacity(p.pos.len());
                let mut n = Vec::with_capacity(p.pos.len());
                for i in 0..p.pos.len() {
                    let (mut acc, mut nacc) = ([0.0f32; 3], [0.0f32; 3]);
                    for k in 0..4 {
                        let wt = p.weights[i][k];
                        if wt == 0.0 { continue; }
                        let m = &jm[(p.joints[i][k] as usize).min(jm.len() - 1)];
                        let a = xpoint(m, p.pos[i]);
                        let b = xdir(m, p.nrm[i]);
                        for c in 0..3 { acc[c] += a[c] * wt; nacc[c] += b[c] * wt; }
                    }
                    v.push(acc);
                    n.push(norm(nacc));
                }
                vs.push(v); ns.push(n);
            } else {
                let m = &w[p.node];
                vs.push(p.pos.iter().map(|&q| xpoint(m, q)).collect());
                ns.push(p.nrm.iter().map(|&q| norm(xdir(m, q))).collect());
            }
        }
        (vs, ns)
    }

    /// Draw the Mii into a `w` x `h` ARGB picture (transparent background).
    /// `fill` = how much of the picture's height the standing Mii takes (0..1).
    pub fn render(&self, pose: &Pose, w: usize, h: usize, fill: f32) -> Vec<u32> {
        let (ss_w, ss_h) = (w * 2, h * 2); // draw at double size, then shrink: smooth edges
        let t0 = std::time::Instant::now();
        let (verts, nrms) = self.posed(pose);
        let t_pose = t0.elapsed();
        let m = &pose.mii;
        let yaw = m.yaw as f32 / 256.0 * std::f32::consts::TAU;
        let (sy, cy) = yaw.sin_cos();
        let pitch: f32 = 0.12; // look slightly down on the Mii
        let (sp, cp) = pitch.sin_cos();
        let center = [(self.min[0] + self.max[0]) * 0.5, self.min[1], (self.min[2] + self.max[2]) * 0.5];
        // Whole-body flip (around the side axis) and roll (around the front axis),
        // pivoting at the middle of the body; then squash & stretch from the feet.
        let (spit, cpit) = (m.pitch as f32 / 256.0 * std::f32::consts::TAU).sin_cos();
        let (srol, crol) = (m.roll as f32 / 256.0 * std::f32::consts::TAU).sin_cos();
        let mid = self.min[1] + self.height() * 0.5;
        let sq = (m.squash.max(64) as f32) / 256.0;
        let sqx = 1.0 / sq.sqrt();
        let body = |p: V3, is_normal: bool| -> V3 {
            let (x, y, z) = if is_normal { (p[0], p[1], p[2]) } else { (p[0] - center[0], p[1] - mid, p[2] - center[2]) };
            let (y, z) = (y * cpit - z * spit, y * spit + z * cpit);
            let (x, y) = (x * crol - y * srol, x * srol + y * crol);
            if is_normal { [x, y, z] } else { [x * sqx + center[0], (y + mid - self.min[1]) * sq + self.min[1], z * sqx + center[2]] }
        };
        let scale = ss_h as f32 * fill / self.height().max(1e-3);
        let light = norm([-0.35, 0.55, 0.75]);
        // model space -> picture space (x right, y down, z towards the viewer)
        let project = |p: V3| -> V3 {
            let p = body(p, false);
            let (x, y, z) = (p[0] - center[0], p[1] - center[1], p[2] - center[2]);
            let (x, z) = (x * cy + z * sy, -x * sy + z * cy);
            let (y, z) = (y * cp - z * sp, y * sp + z * cp);
            [ss_w as f32 * 0.5 + x * scale, ss_h as f32 * 0.97 - y * scale, z * scale]
        };
        let rot = |n: V3| -> V3 { let n = body(n, true); let (x, z) = (n[0] * cy + n[2] * sy, -n[0] * sy + n[2] * cy); [x, n[1] * cp - z * sp, n[1] * sp + z * cp] };

        let mut color = vec![[0f32; 4]; ss_w * ss_h];
        let mut depth = vec![f32::MIN; ss_w * ss_h];
        for pass in 0..2 { // opaque parts first, then see-through ones on top
            for (pi, p) in self.prims.iter().enumerate() {
                let mat = &self.mats[p.mat.min(self.mats.len() - 1)];
                if mat.blend != (pass == 1) { continue; }
                let tex = mat.tex.and_then(|t| self.texs.get(t));
                let sv: Vec<V3> = verts[pi].iter().map(|&v| project(v)).collect();
                for tri in p.idx.chunks_exact(3) {
                    let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
                    let (pa, pb, pc) = (sv[a], sv[b], sv[c]);
                    let area = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]);
                    if area.abs() < 1e-6 { continue; }
                    let x0 = pa[0].min(pb[0]).min(pc[0]).max(0.0) as usize;
                    let x1 = (pa[0].max(pb[0]).max(pc[0]).ceil() as usize).min(ss_w);
                    let y0 = pa[1].min(pb[1]).min(pc[1]).max(0.0) as usize;
                    let y1 = (pa[1].max(pb[1]).max(pc[1]).ceil() as usize).min(ss_h);
                    // Light each corner once; pixels blend between the corners (Gouraud shading).
                    let shade = |n: V3| { let n = rot(n); 0.62 + 0.38 * (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).max(0.0) };
                    let (la, lb, lc) = (shade(nrms[pi][a]), shade(nrms[pi][b]), shade(nrms[pi][c]));
                    // Edge functions, stepped by addition along each row.
                    let inv = 1.0 / area;
                    let (e0x, e1x) = (-(pc[1] - pb[1]) * inv, -(pa[1] - pc[1]) * inv);
                    let (uva, uvb, uvc) = (p.uv[a], p.uv[b], p.uv[c]);
                    for py in y0..y1 {
                        let fy = py as f32 + 0.5;
                        let fx0 = x0 as f32 + 0.5;
                        let w0s = ((pb[0] - fx0) * (pc[1] - fy) - (pb[1] - fy) * (pc[0] - fx0)) * inv;
                        let w1s = ((pc[0] - fx0) * (pa[1] - fy) - (pc[1] - fy) * (pa[0] - fx0)) * inv;
                        // Where along this row is the pixel centre inside all three edges?
                        // Each weight is a straight line in x, so solve for the crossing points.
                        let (mut lo, mut hi) = (0.0f32, (x1 - x0) as f32);
                        for (w, dw) in [(w0s, e0x), (w1s, e1x), (1.0 - w0s - w1s, -(e0x + e1x))] {
                            if dw > 1e-12 { lo = lo.max(-w / dw); } else if dw < -1e-12 { hi = hi.min(-w / dw); } else if w < 0.0 { hi = -1.0; }
                        }
                        if hi < lo { continue; }
                        let (ks, ke) = ((lo.ceil().max(0.0) as usize).saturating_sub(1), ((hi.floor() as usize) + 2).min(x1 - x0));
                        let mut w0 = w0s + e0x * ks as f32;
                        let mut w1 = w1s + e1x * ks as f32;
                        let row = py * ss_w;
                        for px in x0 + ks..x0 + ke {
                            let (c0, c1) = (w0, w1);
                            w0 += e0x; w1 += e1x;
                            let w2 = 1.0 - c0 - c1;
                            if c0 < 0.0 || c1 < 0.0 || w2 < 0.0 { continue; }
                            let z = c0 * pa[2] + c1 * pb[2] + w2 * pc[2];
                            let i = row + px;
                            if pass == 0 { if z < depth[i] { continue; } } else if z < depth[i] - 0.5 { continue; }
                            let mut col = mat.color;
                            if let Some(t) = tex {
                                let u = c0 * uva[0] + c1 * uvb[0] + w2 * uvc[0];
                                let v = c0 * uva[1] + c1 * uvb[1] + w2 * uvc[1];
                                // Mii face textures are half-pictures repeated as a mirror image.
                                let wrap = |c: f32, mirror: bool| if mirror { let m = c.rem_euclid(2.0); if m > 1.0 { 2.0 - m } else { m } } else { c.rem_euclid(1.0) };
                                let tx = (wrap(u, mat.mirror_u) * t.w as f32) as usize % t.w;
                                let ty = (wrap(v, mat.mirror_v) * t.h as f32) as usize % t.h;
                                let s = t.px[ty * t.w + tx];
                                col = [col[0] * s[0] as f32 / 255.0, col[1] * s[1] as f32 / 255.0, col[2] * s[2] as f32 / 255.0, col[3] * s[3] as f32 / 255.0];
                            }
                            let lit = c0 * la + c1 * lb + w2 * lc;
                            let rgb = [col[0] * lit, col[1] * lit, col[2] * lit];
                            if pass == 0 {
                                depth[i] = z;
                                color[i] = [rgb[0], rgb[1], rgb[2], 1.0];
                            } else {
                                let al = col[3];
                                let d = &mut color[i];
                                for k in 0..3 { d[k] = d[k] * (1.0 - al) + rgb[k] * al; }
                                d[3] = d[3].max(al);
                            }
                        }
                    }
                }
            }
        }
        let t_raster = t0.elapsed();
        // Shrink 2x2 -> 1 pixel and pack as ARGB.
        let mut out = vec![0u32; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let c = color[(y * 2 + dy) * ss_w + x * 2 + dx];
                    for k in 0..3 { acc[k] += c[k] * c[3]; }
                    acc[3] += c[3];
                }
                if acc[3] <= 0.0 { continue; }
                let a = acc[3] / 4.0;
                let to8 = |v: f32| ((v / acc[3]).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u32; // linear -> screen colours
                out[y * w + x] = ((a * 255.0) as u32) << 24 | to8(acc[0]) << 16 | to8(acc[1]) << 8 | to8(acc[2]);
            }
        }
        if std::env::var("MIIOS_PROFILE").is_ok() { eprintln!("pose {:?} raster {:?} total {:?}", t_pose, t_raster, t0.elapsed()); }
        out
    }
}

/// Pull `"key":"..."` out of the Mii Creator data without a JSON library.
fn json_str(raw: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\"", key);
    let i = raw.find(&pat)?;
    let rest = &raw[i + pat.len()..];
    let q1 = rest.find('"')?;
    let rest = &rest[q1 + 1..];
    let q2 = rest.find('"')?;
    let n = rest[..q2].trim().to_string();
    if n.is_empty() { None } else { Some(n) }
}
