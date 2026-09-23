//! Diagnostic: report custody preconditions WITHOUT printing any secret value.
//! Answers "why does wallet setup say the envelope is corrupt" by showing which
//! keyring accounts exist and whether the envelope file does.
fn main() {
    let services = ["ai.citrate.core", "ai.citrate.core.custody"];
    let accounts = [
        "custody-master-key",
        "custody-generation",
        "custody-lockout-generation",
        "custody-auto-passphrase",
    ];
    let home = std::env::var("HOME").unwrap_or_default();
    let env_path = std::path::PathBuf::from(home).join(".local/share/ai.citrate.core/custody.enc");
    println!(
        "envelope: {} exists={}",
        env_path.display(),
        env_path.exists()
    );
    for service in services {
        println!("keyring service: {service}");
        for a in accounts {
            match keyring::Entry::new(service, a) {
                Ok(e) => match e.get_secret() {
                    Ok(b) => println!("  {a:32} PRESENT ({} bytes)", b.len()),
                    Err(keyring::Error::NoEntry) => println!("  {a:32} absent"),
                    Err(err) => println!("  {a:32} ERROR: {err}"),
                },
                Err(err) => println!("  {a:32} entry-open ERROR: {err}"),
            }
        }
    }
}
