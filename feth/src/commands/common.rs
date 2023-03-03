use agent::error::Result;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

pub(crate) const UTXO_SECRET: &str = ".utxo.mn.secret";
pub(crate) const ETH_SECRET: &str = ".secret";
pub(crate) const UTXO_SOURCE_FILE: &str = "utxo_source_keys.001";
pub(crate) const ETH_SOURCE_FILE: &str = "source_keys.001";

pub(crate) async fn read_mnemonics<P>(secret: P, mut mnemonics: Vec<String>) -> Result<Vec<String>>
where
    P: AsRef<Path>,
{
    let file = tokio::fs::OpenOptions::new().read(true).open(secret).await?;
    let mut lines = tokio::io::BufReader::new(file).lines();
    while let Some(line) = lines.next_line().await? {
        mnemonics.push(line)
    }

    Ok(mnemonics)
}

pub(crate) async fn write_mnemonics<P>(secret: P, mnemonics: Vec<String>) -> Result<Vec<String>>
where
    P: AsRef<Path>,
{
    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(secret)
        .await?;
    let mut buffer = tokio::io::BufWriter::new(file);
    for mn in &mnemonics {
        buffer.write_all(mn.as_bytes()).await?;
        buffer.write_all(b"\n").await?;
    }
    buffer.flush().await?;

    Ok(mnemonics)
}
