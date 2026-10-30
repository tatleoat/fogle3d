// SPDX-License-Identifier: MIT
//! `skeleton.c`: the one routine of the skeleton code that `SI_envelopCopy` calls. (`si-kine`
//! has the file on a store of its own; this is the same routine on the image.)

use si_mem::{Mem, Ptr32};
use si_types::model::Model;

/// The root of the chain a node belongs to [soft 0x081d20c0 SI_skeChainRoot].
///
/// Null for no node. A null model (kind 0) whose algorithm is 1 or 2 is a chain root and
/// answers itself. A joint (kind 5), or a null model whose algorithm is 3 or 4 (an
/// effector), is followed upwards: from a joint first to the nearest ancestor that is a
/// null model, then, as long as that node's algorithm is neither 1 nor 2, on to the next
/// ancestor that is a null model. Any other node answers itself.
///
/// The fathers are followed without a test for null: the original dies on a chain without
/// root. Here that read is the recorded fault and the answer is null.
pub fn ske_chain_root(m: &Mem, model: Ptr32<Model>) -> Ptr32<Model> {
    if model.is_null() {
        return model;
    }
    let mut node = model;
    let kind = m[node].kind;
    if kind == 0 && (m[node].algo_type as u32).wrapping_sub(1) <= 1 {
        return node;
    }
    if kind != 5 && (kind != 0 || (m[node].algo_type as u32).wrapping_sub(3) > 1) {
        return node;
    }
    let mut up = kind != 0;
    loop {
        if up {
            loop {
                node = m[node].father;
                if node.is_null() {
                    // the original reads the kind at address 0x30 here
                    let _ = m[node].kind;
                    return node;
                }
                if m[node].kind == 0 {
                    break;
                }
            }
        }
        if (m[node].algo_type as u32).wrapping_sub(1) <= 1 {
            return node;
        }
        up = true;
    }
}
