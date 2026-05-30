// Build the gRPC client stubs from the shared proto definition. Same proto crates/creda-core
// uses, so the wire types stay in lockstep.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = "../../../crates/creda-core/proto/creda.proto";
    let proto_dir = "../../../crates/creda-core/proto";
    tonic_build::configure()
        .build_server(false) // client only
        .compile_protos(&[proto], &[proto_dir])?;
    println!("cargo:rerun-if-changed={}", proto);
    Ok(())
}
