pub mod download;
pub mod model;
pub mod ops;
pub mod tensor;
use crate::download::download;
use crate::model::Gpt2;
use crate::ops::special_argmax;
use memmap2::MmapOptions;
use safetensors::tensor::{SafeTensorError, SafeTensors};
use std::fs::File;
use thiserror::Error;
use tokenizers::Tokenizer;

#[derive(Debug, Error)]
pub enum Gpt2Error {
    #[error("i/o error")]
    IOError(#[from] std::io::Error),
    #[error("safetensor error")]
    SafeTensorError(#[from] SafeTensorError),
    #[error("slice error")]
    Slice(#[from] std::array::TryFromSliceError),
    #[error("parsing int error")]
    ParseIntError(#[from] core::num::ParseIntError),
    #[error("reqwest int error")]
    RequestError(#[from] reqwest::Error),
    #[error("reqwest header cannot be parsed error")]
    HeaderToStrError(#[from] reqwest::header::ToStrError),
    #[error("Cannot acquire semaphore")]
    AcquireError(#[from] tokio::sync::AcquireError),
    #[error("No content length")]
    NoContentLength,
}

pub async fn run() -> Result<(), Gpt2Error> {
    let start = std::time::Instant::now();
    let filename = "model.safetensors";
    let max_files = 100;
    let chunk_size = 10_000_000;
    if !std::path::Path::new(filename).exists() {
        let url = "https://huggingface.co/gpt2/resolve/main/model.safetensors";
        println!("Downloading {url:?} into {filename:?}");
        download(url, filename, max_files, chunk_size).await?;
    }
    let file = File::open(filename)?;
    let buffer = unsafe { MmapOptions::new().map(&file)? };
    let tensors = SafeTensors::deserialize(&buffer)?;
    println!("Safetensors {:?}", start.elapsed());

    let filename = "tokenizer.json";
    if !std::path::Path::new(filename).exists() {
        let url = "https://huggingface.co/gpt2/resolve/main/tokenizer.json";
        println!("Downloading {url:?} into {filename:?}");
        download(url, filename, max_files, chunk_size).await?;
    }
    let tokenizer = Tokenizer::from_file(filename).unwrap();
    println!("Tokenizer {:?}", start.elapsed());

    let num_heads = 12;
    let gpt2 = Gpt2::from_tensors(&tensors, num_heads);
    let string = "My name is";

    let encoded = tokenizer.encode(string, false).unwrap();
    println!("Loaded & encoded {:?}", start.elapsed());
    let mut ids = encoded.get_ids().to_vec();
    let mut past_key_values = gpt2.empty_past_key_values();
    let mut current_ids = ids.clone();
    for _i in 0..10 {
        let start = std::time::Instant::now();
        let logits = gpt2.forward(&current_ids, &mut past_key_values).await;
        let new_id = special_argmax(&logits);
        ids.push(new_id as u32);
        current_ids = vec![new_id as u32];
        println!("Loop in {:?}", start.elapsed());
    }
    println!("Result {:?}", tokenizer.decode(ids, false));
    println!("Total Inference {:?}", start.elapsed());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tensor::Tensor;

    pub(crate) fn simplify(data: &[f32]) -> Vec<f32> {
        let precision = 3;
        let m = 10.0 * 10.0f32.powf(precision as f32);
        data.iter().map(|x| (x * m).round() / m).collect()
    }

    fn assert_float_eq(left: &[f32], right: &[f32]) {
        assert_eq!(left.len(), right.len());

        left.iter().zip(right.iter()).for_each(|(l, r)| {
            assert!(
                (l - r).abs() / l.abs() < 1e-4,
                "{l} != {r}\n{left:?}\n{right:?}"
            );
        });
    }

    #[test]
    fn simple_logits() {
        let num_heads = 12;
        let filename = "model.safetensors";
        let file = File::open(filename).unwrap();
        let buffer = unsafe { MmapOptions::new().map(&file).unwrap() };
        let tensors = SafeTensors::deserialize(&buffer).unwrap();

        let filename = "tokenizer.json";
        let tokenizer = Tokenizer::from_file(filename).unwrap();
        let gpt2 = Gpt2::from_tensors(&tensors, num_heads);
        let string = "My name is";
        let encoded = tokenizer.encode(string, false).unwrap();
        let mut current_ids = encoded.get_ids().to_vec();
        let mut past_key_values = gpt2.empty_past_key_values();
        let logits = gpt2.forward(&current_ids, &mut past_key_values);
        assert_float_eq(
            &logits.data()[..10],
            &[
                -33.0735, -32.3349, -35.2380, -34.7751, -33.8666, -34.4521, -33.0241, -33.5888,
                -32.0457, -34.4161,
            ],
        );
        assert_float_eq(
            &logits.data()[logits.data().len() - 10..],
            &[
                -77.3382, -73.0993, -80.6285, -78.5444, -79.3092, -79.2024, -76.1651, -78.1296,
                -77.4711, -71.8745,
            ],
        );

        let new_id = special_argmax(&logits);
        current_ids = vec![new_id as u32];
        let logits = gpt2.forward(&current_ids, &mut past_key_values);
        assert_float_eq(
            &logits.data()[..10],
            &[
                -70.2707, -70.1531, -75.9321, -76.6249, -75.0689, -74.5452, -72.4047, -73.3955,
                -72.8820, -73.8592,
            ],
        );
        assert_float_eq(
            &logits.data()[logits.data().len() - 10..],
            &[
                -78.4766, -75.4068, -83.2028, -85.4337, -83.8543, -84.6238, -78.8617, -83.8258,
                -81.3094, -72.5672,
            ],
        );
    }
}
