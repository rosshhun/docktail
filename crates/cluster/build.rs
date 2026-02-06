fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile the protobuf file into Rust code
    tonic_prost_build::configure()
        .build_client(true)  // Cluster is gRPC client only
        .build_server(false)
        .compile_protos(&["../agent/proto/agent.proto"], &["../agent/proto"])?;
    
    Ok(())
}
