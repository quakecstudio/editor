// SPDX-License-Identifier: GPL-3.0-or-later

//! Test fixtures: factories that write MD5 mesh and animation files as the spec's vectors do.

/// A mesh file to write. The default is the first row of the MD5-3 vectors: one joint, one
/// mesh of 3 vertices, 1 triangle and 1 weight.
#[derive(Clone, Debug)]
pub(crate) struct MeshSpec {
    pub(crate) num_joints: &'static str,
    pub(crate) num_meshes: &'static str,
    /// Lines inside `joints { }`.
    pub(crate) joints: Vec<String>,
    /// Each mesh's body, inside `mesh { }`.
    pub(crate) meshes: Vec<String>,
    pub(crate) trailer: &'static str,
}

/// The default mesh body with `tri` replacing its triangle line.
pub(crate) fn mesh_with_tri(tri: &str) -> String {
    format!(
        "shader \"ogre\"\nnumverts 3\nvert 0 ( 0 0 ) 0 1\nvert 1 ( 1 0 ) 0 1\n\
         vert 2 ( 0.5 1 ) 0 1\nnumtris 1\n{tri}\nnumweights 1\nweight 0 0 1 ( 0 0 0 )\n"
    )
}

impl Default for MeshSpec {
    fn default() -> Self {
        Self {
            num_joints: "1",
            num_meshes: "1",
            joints: vec!["\"root\" -1 ( 1 2 3 ) ( 0 0 0 )".to_owned()],
            meshes: vec![mesh_with_tri("tri 0 0 1 2")],
            trailer: "",
        }
    }
}

impl MeshSpec {
    pub(crate) fn build(&self) -> Vec<u8> {
        let mut text = format!(
            "MD5Version 10\ncommandline \"exported\"\nnumJoints {}\nnumMeshes {}\n\njoints {{\n",
            self.num_joints, self.num_meshes
        );
        for joint in &self.joints {
            text.push_str(&format!("\t{joint}\n"));
        }
        text.push_str("}\n");
        for mesh in &self.meshes {
            text.push_str(&format!("\nmesh {{\n{mesh}}}\n"));
        }
        text.push_str(self.trailer);
        text.into_bytes()
    }
}

/// An animation file to write. The default is the MD5-3 vector: 2 frames, one joint
/// `root` with flags 63 from component 0, 6 components, and frame 1 only.
#[derive(Clone, Debug)]
pub(crate) struct AnimSpec {
    pub(crate) num_frames: &'static str,
    pub(crate) num_joints: &'static str,
    pub(crate) num_components: &'static str,
    /// Lines inside `hierarchy { }`.
    pub(crate) hierarchy: Vec<String>,
    /// Each frame's index and its components.
    pub(crate) frames: Vec<(String, String)>,
    pub(crate) trailer: &'static str,
}

impl Default for AnimSpec {
    fn default() -> Self {
        Self {
            num_frames: "2",
            num_joints: "1",
            num_components: "6",
            hierarchy: vec!["\"root\" -1 63 0".to_owned()],
            frames: vec![("1".to_owned(), "1 2 3 0.6 0 0".to_owned())],
            trailer: "",
        }
    }
}

impl AnimSpec {
    pub(crate) fn build(&self) -> Vec<u8> {
        let mut text = format!(
            "MD5Version 10\ncommandline \"\"\nnumFrames {}\nnumJoints {}\nframeRate 10\n\
             numAnimatedComponents {}\n\nhierarchy {{\n",
            self.num_frames, self.num_joints, self.num_components
        );
        for entry in &self.hierarchy {
            text.push_str(&format!("\t{entry}\n"));
        }
        text.push_str(
            "}\n\nbounds {\n\t( -1 -1 -1 ) ( 1 1 1 )\n\t( -2 -2 -2 ) ( 2 2 2 )\n}\n\n\
             baseframe {\n\t( 0 0 0 ) ( 0 0 0 )\n}\n",
        );
        for (index, components) in &self.frames {
            text.push_str(&format!("\nframe {index} {{\n\t{components}\n}}\n"));
        }
        text.push_str(self.trailer);
        text.into_bytes()
    }
}
