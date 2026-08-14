use ort::session::Session;

fn main() -> Result<(), ort::Error> {
    let session = Session::builder()?.commit_from_file("assets/models/silero_vad.onnx")?;
    println!("inputs: {:#?}", session.inputs());
    println!("outputs: {:#?}", session.outputs());
    Ok(())
}
