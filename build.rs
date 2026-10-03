use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=CUDA_PATH");
    let windows = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if windows && env::var_os("CARGO_FEATURE_CUDA").is_some() && env::var_os("CUDA_PATH").is_none() {
        panic!(
            "\n\nThe cuda feature needs the NVIDIA CUDA Toolkit, and CUDA_PATH is not set.\n\
             Install it with `winget install Nvidia.CUDA` (after the Visual Studio Build Tools),\n\
             then open a new terminal so CUDA_PATH is picked up. Or build without `--features cuda`.\n\n"
        );
    }
}
