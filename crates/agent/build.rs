fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile the protobuf file into Rust code
    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(&["proto/agent.proto"], &["proto"])?;
    
    Ok(())
}
