fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::compile_protos("proto/risk_engine.proto")?;
    Ok(())
}
