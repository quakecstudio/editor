// SPDX-License-Identifier: GPL-3.0-or-later
//! temp timing
use std::time::Instant;
fn main() {
    let map = std::fs::read(std::env::args().nth(1).unwrap_or_default()).unwrap_or_default();
    let compiler = qs_map_bsp::Compiler::default().with_dialect(qs_map_bsp::Dialect::Extended);
    let t = Instant::now();
    let compiled = compiler.compile(&map, None).unwrap();
    println!("bsp {:?}", t.elapsed());
    let t = Instant::now();
    let fast = qs_map_light::Light::new(qs_map_light::Sampling::Fast).run(&compiled.bsp).unwrap();
    println!("fast light (no vis) {:?} lit {}", t.elapsed(), fast.lit.is_some());
    let portals = compiled.portal_file.unwrap();
    let t = Instant::now();
    let seen = qs_map_vis::Vis::new(qs_map_vis::Mode::default()).run(&compiled.bsp, &String::from_utf8_lossy(&portals)).unwrap();
    println!("vis {:?}", t.elapsed());
    let t = Instant::now();
    let _ = qs_map_light::Light::new(qs_map_light::Sampling::Extra).run(&seen).unwrap();
    println!("extra light {:?}", t.elapsed());
    let t = Instant::now();
    let _ = qs_map_light::Light::new(qs_map_light::Sampling::Fast).with_dialect(qs_map_bsp::Dialect::Id).run(&seen);
    println!("id fast light {:?}", t.elapsed());
}
