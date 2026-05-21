//! Build script. Compiles the gRPC `.proto` to Rust **only** when the `grpc` feature is enabled
//! (Cargo sets `CARGO_FEATURE_GRPC` then). Default builds skip this entirely, so `protoc` is not
//! required unless you build with `--features grpc`.

fn main() {
    if std::env::var("CARGO_FEATURE_GRPC").is_ok() {
        println!("cargo:rerun-if-changed=proto/creda.proto");
        tonic_build::configure()
            .build_server(true)
            .build_client(false)
            .compile_protos(&["proto/creda.proto"], &["proto"])
            .expect("failed to compile creda.proto (is protoc installed?)");
    }
}
