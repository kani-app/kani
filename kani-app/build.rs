fn main() {
    println!("cargo:rerun-if-changed=../migrations");
    println!("cargo:rerun-if-changed=proto/tachiyomi_backup.proto");

    let fds = protox::compile(["proto/tachiyomi_backup.proto"], ["proto/"])
        .expect("failed to compile tachiyomi_backup.proto");
    prost_build::Config::new()
        .compile_fds(fds)
        .expect("failed to generate prost bindings from tachiyomi_backup.proto");
}
