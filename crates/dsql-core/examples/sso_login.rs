//! Manual check of the SSO device flow: signs in, lists the reachable accounts and roles, and writes the token to a
//! cache directory you choose (never the real `~/.aws/sso/cache` unless you pass it).
//!
//! Usage: `cargo run -p dsql-core --example sso_login -- <start_url> <sso_region> <session_name> <cache_dir>`

use dsql_core::profiles::AwsPaths;
use dsql_core::sso;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [start_url, sso_region, session, cache_dir] = args.as_slice() else {
        eprintln!("usage: sso_login <start_url> <sso_region> <session_name> <cache_dir>");
        std::process::exit(2);
    };

    let login = sso::start_device_login(start_url, sso_region).await?;
    println!("open: {}", login.verification_uri_complete);
    println!("code: {}", login.user_code);
    let token = sso::finish_device_login(&login, start_url, sso_region).await?;
    println!("signed in; token expires at {}", token.expires_at);

    for r in sso::list_accounts_and_roles(&token).await? {
        println!(
            "account {} ({}) role {}",
            r.account_name, r.account_id, r.role_name
        );
    }

    let paths = AwsPaths {
        config: "unused".into(),
        credentials: "unused".into(),
        sso_cache: cache_dir.into(),
    };
    let key = sso::sso_cache_key(Some(session), start_url);
    sso::write_sso_cache(&paths, &key, &token)?;
    println!(
        "wrote {}",
        paths.sso_cache.join(format!("{key}.json")).display()
    );
    Ok(())
}
