use std::env;

pub async fn remote_public_key() -> String {
    let url = env::var("REMOTE_PUBLIC_KEY").expect("missing REMOTE PUBLIC KEY");
    let response = reqwest::get(url)
        .await
        .expect("error occured in getting remote public key");
    response
        .text()
        .await
        .expect("could not get text content of response")
}
