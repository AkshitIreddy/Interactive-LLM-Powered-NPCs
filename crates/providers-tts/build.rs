fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path()
        .expect("the pinned vendored protoc package must contain this build target");
    // Cargo runs build scripts in their own process before rustc starts. This
    // process-local setting is the supported protoc-bin-vendored integration.
    std::env::set_var("PROTOC", protoc);
    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(&["proto/riva/proto/riva_tts.proto"], &["proto"])
        .expect("the checked-in NVIDIA Riva protobuf contract must compile");
    println!("cargo:rerun-if-changed=proto/riva/proto/riva_tts.proto");
    println!("cargo:rerun-if-changed=proto/riva/proto/riva_audio.proto");
    println!("cargo:rerun-if-changed=proto/riva/proto/riva_common.proto");
}
