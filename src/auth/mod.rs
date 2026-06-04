pub mod device_code;
pub mod poll_token;

use anyhow::Result;

use crate::config;

/// Run the full device authorization flow and persist the resulting GitHub token.
pub async fn run_device_flow(http: &reqwest::Client) -> Result<String> {
    let device = device_code::request_device_code(http).await?;

    println!();
    println!("Please open the following URL in your browser and enter the code:");
    println!("  URL:  {}", device.verification_uri);
    println!("  Code: {}", device.user_code);
    println!();
    println!("Waiting for authorization...");

    let token = poll_token::poll_access_token(http, &device).await?;
    config::save_github_token(&token)?;
    println!("Authorization successful. Token saved to {:?}", config::github_token_path()?);

    Ok(token)
}
