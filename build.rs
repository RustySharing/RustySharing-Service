// build script for cargo
// here we configure tonic

fn main() -> Result<(), Box<dyn std::error::Error>> {
  tonic_build::compile_protos("proto/image_encoding.proto")?;
  tonic_build::compile_protos("proto/leader_provider.proto")?;
  Ok(())
}
