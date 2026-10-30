// SPDX-License-Identifier: MIT
//! Records of the three source files that `si-types` does not lay out (`record!` layouts,
//! offsets asserted at compile time). Their fields are as the code of `cluster.c` uses
//! them (finding 0030); a field no routine of this crate's ports reads is named `unk_..`.

use si_mem::{record, CStr32, Opaque, Ptr32};

record! {
    /// A model's cluster animation (`SI_Cluster`, 0xc bytes from `SI_malloc`), at +0x344 of
    /// the model node.
    pub struct Cluster = "SI_Cluster", size 0x0c {
        0x00 block: Ptr32<Opaque>, "a block freed with the record (SI_clusterDispose); 0 at creation";
        0x04 active: i32, "0 at creation; SI_modelActiveCluster turns it between 0 and 1";
        0x08 unk_08: i16, "0 at creation";
        0x0a pad_0a: [u8; 2], "never written";
    }
}

record! {
    /// The shape keys of one cluster (`SI_ClusterData`, 0x1c bytes from `SI_malloc`), at
    /// +0x18 of the cluster.
    pub struct ClusterData = "SI_ClusterData", size 0x1c {
        0x00 active: i32, "1 at creation; the cluster's keys move its points only while it is 1 (SI_clusterActive turns it)";
        0x04 current: Ptr32<ClusterKey>, "0 at creation; the key clusterKeyNumber looks for in the chain";
        0x08 interp: u16, "0 at creation; 0 linear between two keys, 1 cardinal through four, 2 weighted";
        0x0a unk_0a: [u8; 0x0a], "never written at creation";
        0x14 nb_keys: i32, "0 at creation: how many keys the chain has";
        0x18 keys: Ptr32<ClusterKey>, "the chain of keys, 0 at creation";
    }
}

record! {
    /// One key of a cluster (`clusterKey`, 8 bytes from `SI_malloc`).
    pub struct ClusterKey = "clusterKey", size 0x08 {
        0x00 points: Ptr32<Opaque>, "one position per point of the cluster: 12 bytes for a mesh, 16 for a patch or NURBS kind, 36 for a face or spline";
        0x04 next: Ptr32<ClusterKey>, "the next key, 0 at creation";
    }
}

record! {
    /// A cluster of points (`kluster`, 0x20 bytes) as finding 0028 has it (`si-deform`
    /// lays it out the same; repeated here so that this crate does not depend on that one).
    pub struct Kluster = "kluster", size 0x20 {
        0x00 name: CStr32, "the name";
        0x04 count: i32, "how many points";
        0x08 cursor: i32, "the place from which the cluster interpolators look for the next point (SI_modelKlusterResetCluInd clears it)";
        0x0c indexes: Ptr32<i32>, "the point numbers, ascending (two words a point for a face)";
        0x10 state: i32, "0 active, 1 not active, 2 to be disposed, 3, 4";
        0x14 unk_14: u32, "not read here";
        0x18 anim: Ptr32<ClusterData>, "the cluster's shape keys";
        0x1c next: Ptr32<Kluster>, "the next cluster of the model";
    }
}
