#[test]
#[cfg(not(kmux_reference))]
fn the_kmux_reference_is_present() {
    let dir = include_str!(concat!(env!("OUT_DIR"), "/kmux_dir.txt"));
    panic!("kmux is the reference model for the kanna CLI, and its source isn't at {dir} (references/kmux, or KMUX_DIR)");
}
