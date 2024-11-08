use image::{imageops, GenericImageView};
use serde::{Deserialize, Serialize};
use serde_json::to_vec;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::str;
use stegano_core::commands::unveil;
use stegano_core::{CodecOptions, SteganoCore, SteganoEncoder};
use steganography::encoder;
use steganography::util::file_to_bytes;

fn create_directory_if_not_exists(dir_path: &str) -> std::io::Result<()> {
    // Convert the dir_path to a Path
    let path = Path::new(dir_path);

    // Create the directory (and any parent directories) if it doesn't exist
    fs::create_dir_all(path)?;

    println!("Directory '{}' created or already exists.", dir_path);

    Ok(())
}

fn calculate_image_dimensions_from_data(
    data_size: usize,
    bits_per_pixel: u32,
    aspect_ratio: (u32, u32),
) -> (u32, u32) {
    // Calculate the total number of bits needed
    let total_bits_needed = data_size * 8; // Convert bytes to bits

    // Calculate the number of pixels needed
    let pixels_needed = (total_bits_needed + bits_per_pixel as usize - 1) / bits_per_pixel as usize; // Round up

    // Aspect ratio
    let (aspect_width, aspect_height) = aspect_ratio;

    // Calculate the total aspect ratio
    let total_aspect = aspect_width as f32 / aspect_height as f32;

    // Calculate width and height based on the aspect ratio
    let height = (pixels_needed as f32 / total_aspect.sqrt()).sqrt().round() as u32;
    let width = (height as f32 * total_aspect).round() as u32;

    (width, height)
}

fn calculate_aspect_ratio(width: u32, height: u32) -> (u32, u32) {
    // Calculate the greatest common divisor (GCD) using the Euclidean algorithm
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }

    let divisor = gcd(width, height);
    (width / divisor, height / divisor)
}

// Function to encode an image with embedded data
pub fn encode_image(
    img: Vec<u8>,
) -> Result<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>, Box<dyn Error>> {
    print!("Started Encoding!");

    // let data = "Hello, steganography!\n2024-10-27T00:00:00Z".to_string();

    // let mut file = File::create("input.txt")?;

    // file.write_all(data.as_bytes())?;

    let img_path = "input.jpeg"; // File to embed

    // let text_path = "input.txt";
    let input_paths: Vec<&str> = [img_path /*, text_path */].to_vec();
    let media_path = "carrier.png"; // Stego image (carrier)
    let output_path = "image-with-a-file-inside.png"; // Output image with hidden file
    let extraction_path = "./extracted"; // Path to save extracted image

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

    if let Err(e) = create_directory_if_not_exists(extraction_path) {
        eprintln!("Error creating directory: {}", e);
    }

    // Embed the input file into the image
    SteganoCore::encoder()
        .hide_files(input_paths) // Hide the file (input.jpeg)
        .use_media(media_path) // Use the stego carrier (double.png)
        .unwrap() // Unwrap to panic if anything fails
        .write_to(output_path) // Save the output image with hidden file
        .hide(); // Perform the hiding operation

    println!("Image with embedded data saved to: {}", output_path);

    // Extract the hidden file from the image
    let _ = unveil(
        Path::new(output_path),
        Path::new(extraction_path),
        &CodecOptions::default(),
    );

    println!("Extracted file saved to: {}", extraction_path);

    // Return the encoded image data if successful
    Ok(extraction_path)
}
