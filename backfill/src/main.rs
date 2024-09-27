use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anchor_gen::Decode;
use clap::Parser;
use log::*;

use common::{init_logger, ArchiveAccount, ChannelEvent};
use config::*;
use gcs::bucket::*;
use snapshot::stream_archived_accounts;

// use rayon::prelude::{IntoParallelIterator, ParallelIterator};

mod config;
mod errors;

#[derive(Parser, Debug)]
struct Args {
    /// Path to backfill.yaml config.
    /// Should deserialize into BackfillConfig
    #[arg(long, env, default_value = "backfill.yaml")]
    config_file_path: PathBuf,
}

fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    init_logger();
    let args: Args = Args::parse();
    info!("🏗️ Backfill snapshots with args: {:?}", args);

    let config = BackfillConfig::read_config(&args.config_file_path)?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let bucket = config.gcs_bucket;
    let metas: Vec<SnapshotMeta> = rt.block_on(async move {
        let metas = match &config.gcs_local_file {
            Some(path) => match Path::new(path).exists() {
                false => get_snapshot_metas(GcsObjectsSource::Url(bucket)).await,
                true => get_snapshot_metas(GcsObjectsSource::Path(path.clone())).await,
            },
            None => get_snapshot_metas(GcsObjectsSource::Url(bucket)).await,
        }?;
        Result::<_, anyhow::Error>::Ok(metas)
    })?;
    info!(
        "GCS snapshots found: {} - {}",
        metas.first().unwrap().snapshot.slot,
        metas.last().unwrap().snapshot.slot
    );

    // slice SnapshotMetas to range config wants to backfill
    let start = config.start_slot;
    let end = config.end_slot;
    let metas: Vec<_> = metas
        .into_iter()
        .filter(|m| m.bounds.start_slot >= start && m.bounds.end_slot <= end)
        .collect();
    info!(
        "Snapshot date range to backfill: {} - {}",
        metas.first().unwrap().datetime(),
        metas.last().unwrap().datetime()
    );

    let (tx, rx) = crossbeam_channel::unbounded::<ChannelEvent<ArchiveAccount>>();
    let programs = Arc::new(config.programs);

    // consume decoded accounts from channel
    rt.spawn(async move {
        let programs = programs.clone();

        let mut total_read = 0;

        // upsert accounts to Timescale
        while let Ok(msg) = rx.recv() {
            match msg {
                ChannelEvent::Msg(account) => {
                    if programs.contains(&account.owner) {
                        total_read += 1;
                        // TODO: do something with the account

                        // TODO: example impl of decoding accounts via anchor-gen bindings from an IDL
                        if account.owner == drift_cpi::id() {
                            if let Ok(decoded_acct) =
                                drift_cpi::AccountType::decode(account.data.as_slice())
                                    .map_err(|e| anyhow::anyhow!(e.to_string()))
                            {
                                match decoded_acct {
                                    drift_cpi::AccountType::SpotMarket(acct) => {
                                        info!("Decoded SpotMarket account");
                                    }
                                    drift_cpi::AccountType::User(acct) => {
                                        info!("Decoded User account");
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                ChannelEvent::Done => {
                    info!("Snapshot done, total accounts processed: {}", total_read);
                }
            }
        }
    });

    // backfill from the most recent date to the oldest
    let sender = Arc::new(tx);
    for meta in metas {
        let source = meta.snapshot.url.clone();
        info!("Backfilling snapshot: {:#?}", &meta.datetime());
        match stream_archived_accounts(source, sender.clone()) {
            Ok(_) => {
                info!(
                    "Done snapshot {} for slots {} - {}",
                    &meta.datetime(),
                    &meta.bounds.start_slot,
                    &meta.bounds.end_slot
                );
            }
            Err(e) => {
                error!("Error backfilling snapshot: {}", e);
            }
        }
    }
    Ok(())
}
