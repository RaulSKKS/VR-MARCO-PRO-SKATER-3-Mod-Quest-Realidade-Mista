//! The Marco VR build's version, shown in the menus (the Quest's panel, the PC's game menu and window title) so
//! players can tell builds apart. Bump it with every release, together with the kit's VERSAO.txt.
pub(crate) const VERSION: &str = "1.0.0";
pub(crate) const LABEL: &str = "MarcoVR v1.0.0";

#[cfg(test)]
mod tests {
    #[test]
    fn label_names_the_version() {
        assert!(super::LABEL.ends_with(super::VERSION));
    }
}
