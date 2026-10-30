// SPDX-License-Identifier: MIT
// Chooses the backend of F80's operations: the processor's x87 unit (src/hw.rs) by default on x86-64
// (owner's decision 2026-10-29: it is byte-identical to the software model, finding 0008 "Where it
// stands"), the software model on every other architecture. SI_X87=soft forces the software model;
// SI_X87=hw forces the hardware path and is an error on a non-x86-64 target. Nothing here reproduces a
// function of the original. No target-cpu is involved: the x87 unit is part of the x86-64 baseline, and
// hw.rs reads the control word before every operation and answers from the software model when it is
// not the one the model computes in (0x037f).
fn main() {
    println!("cargo:rustc-check-cfg=cfg(hw_x87)");
    println!("cargo:rerun-if-env-changed=SI_X87");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let choice = std::env::var("SI_X87").unwrap_or_default();
    let x86 = arch == "x86_64";
    match choice.as_str() {
        "" | "soft" => {}
        "hw" if !x86 => panic!("SI_X87=hw: the hardware x87 path exists only on x86_64 (target is {arch})"),
        "hw" => {}
        other => panic!("SI_X87={other}: expected soft or hw"),
    }
    if x86 && choice != "soft" {
        println!("cargo:rustc-cfg=hw_x87");
    }
}
