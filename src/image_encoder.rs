use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::str;
use stegano_core::SteganoCore;
use steganography::util::bytes_to_file;
use uuid::Uuid;

pub fn encode_image(img: Vec<u8>, file_name: &str) -> Result<String, Box<dyn Error>> {
    print!("Started Encoding ... ");

    // let data = "Hello, steganography!\n2024-10-27T00:00:00Z".to_string();
    let img_path = file_name; // File to embed
    let file = File::create(img_path)?;

    bytes_to_file(&img, &file);

    // let text_path = "input.txt";
    let input_paths: Vec<&str> = [img_path /*, text_path */].to_vec();
    let media_path = "carrier.png"; // Stego image (carrier)
    let my_uuid = Uuid::now_v6(&[1, 2, 3, 4, 5, 6]);
    let output_path = &format!("./encoded/{}.png", my_uuid); // Output image with hidden file

    // Ensure the source file (input.jpeg) and carrier image (double.png) exist
    if !Path::new(img_path).exists() {
        eprintln!("Error: The file to hide ({}) does not exist!", img_path);
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "File not found",
        )));
    }

    if !Path::new(media_path).exists() {
        eprintln!("Error: The carrier image ({}) does not exist!", media_path);
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Carrier image not found",
        )));
    }

    // Embed the input file into the image
    SteganoCore::encoder()
        .hide_files(input_paths) // Hide the file (input.jpeg)
        .use_media(media_path) // Use the stego carrier (double.png)
        .unwrap() // Unwrap to panic if anything fails
        .write_to(output_path) // Save the output image with hidden file
        .hide(); // Perform the hiding operation

    println!("Image with embedded data saved to: {}", output_path);

    Ok(output_path.to_string())
}
