#[test]
#[cfg(not(kmux_reference))]
fn the_kmux_reference_is_present() {
    let dir = include_str!(concat!(env!("OUT_DIR"), "/kmux_dir.txt"));
    panic!("kmux is the reference model for the kanna CLI, and there is no kmux checkout at {dir} (clone it beside this repo or set KMUX_DIR)");
}
