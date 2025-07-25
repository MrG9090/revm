//! Example that show how to replay a block and trace the execution of each transaction.
//!
//! The EIP3155 trace of each transaction is saved into file `traces/{tx_number}.json`.
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

use alloy_consensus::Transaction;
use alloy_eips::{BlockId, BlockNumberOrTag};
use alloy_provider::{network::primitives::BlockTransactions, Provider, ProviderBuilder};
use indicatif::ProgressBar;
use revm::{
    context::TxEnv,
    database::{AlloyDB, CacheDB, StateBuilder},
    database_interface::WrapDatabaseAsync,
    inspector::{inspectors::TracerEip3155, InspectEvm},
    primitives::{TxKind, U256},
    Context, MainBuilder, MainContext,
};
use std::fs::create_dir_all;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

struct FlushWriter {
    writer: Arc<Mutex<BufWriter<std::fs::File>>>,
}

impl FlushWriter {
    fn new(writer: Arc<Mutex<BufWriter<std::fs::File>>>) -> Self {
        Self { writer }
    }
}

impl Write for FlushWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.lock().unwrap().flush()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    create_dir_all("traces")?;

    // Set up the HTTP transport which is consumed by the RPC client.
    let rpc_url = "https://mainnet.infura.io/v3/c60b0bb42f8a4c6481ecd229eddaca27".parse()?;

    // Create a provider
    let client = ProviderBuilder::new().connect_http(rpc_url);

    // Params
    let chain_id: u64 = 1;
    let block_number = 10889447;

    // Fetch the transaction-rich block
    let block = match client
        .get_block_by_number(BlockNumberOrTag::Number(block_number))
        .full()
        .await
    {
        Ok(Some(block)) => block,
        Ok(None) => anyhow::bail!("Block not found"),
        Err(error) => anyhow::bail!("Error: {:?}", error),
    };
    println!("Fetched block number: {}", block.header.number);
    let previous_block_number = block_number - 1;

    // Use the previous block state as the db with caching
    let prev_id: BlockId = previous_block_number.into();
    // SAFETY: This cannot fail since this is in the top-level tokio runtime

    let state_db = WrapDatabaseAsync::new(AlloyDB::new(client, prev_id)).unwrap();
    let cache_db: CacheDB<_> = CacheDB::new(state_db);
    let mut state = StateBuilder::new_with_database(cache_db).build();
    let ctx = Context::mainnet()
        .with_db(&mut state)
        .modify_block_chained(|b| {
            b.number = U256::from(block.header.number);
            b.beneficiary = block.header.beneficiary;
            b.timestamp = U256::from(block.header.timestamp);

            b.difficulty = block.header.difficulty;
            b.gas_limit = block.header.gas_limit;
            b.basefee = block.header.base_fee_per_gas.unwrap_or_default();
        })
        .modify_cfg_chained(|c| {
            c.chain_id = chain_id;
        });

    let write = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("traces/0.json");
    let inner = Arc::new(Mutex::new(BufWriter::new(
        write.expect("Failed to open file"),
    )));
    let writer = FlushWriter::new(Arc::clone(&inner));
    let mut evm = ctx.build_mainnet_with_inspector(TracerEip3155::new(Box::new(writer)));

    let txs = block.transactions.len();
    println!("Found {txs} transactions.");

    let console_bar = Arc::new(ProgressBar::new(txs as u64));
    let start = Instant::now();

    // Create the traces directory if it doesn't exist
    std::fs::create_dir_all("traces").expect("Failed to create traces directory");

    // Fill in CfgEnv
    let BlockTransactions::Full(transactions) = block.transactions else {
        panic!("Wrong transaction type")
    };

    for tx in transactions {
        // Construct the file writer to write the trace to
        let tx_number = tx.transaction_index.unwrap_or_default();

        let tx = TxEnv::builder()
            .caller(tx.inner.signer())
            .gas_limit(tx.gas_limit())
            .gas_price(tx.gas_price().unwrap_or(tx.inner.max_fee_per_gas()))
            .value(tx.value())
            .data(tx.input().to_owned())
            .gas_priority_fee(tx.max_priority_fee_per_gas())
            .chain_id(Some(chain_id))
            .nonce(tx.nonce())
            .access_list(tx.access_list().cloned().unwrap_or_default())
            .kind(match tx.to() {
                Some(to_address) => TxKind::Call(to_address),
                None => TxKind::Create,
            })
            .build()
            .unwrap();

        let file_name = format!("traces/{tx_number}.json");
        let write = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(file_name);
        let inner = Arc::new(Mutex::new(BufWriter::new(
            write.expect("Failed to open file"),
        )));
        let writer = FlushWriter::new(Arc::clone(&inner));

        // Inspect and commit the transaction to the EVM
        let res: Result<_, _> = evm.inspect_one(tx, TracerEip3155::new(Box::new(writer)));

        if let Err(error) = res {
            println!("Got error: {error:?}");
        }

        // Flush the file writer
        inner.lock().unwrap().flush().expect("Failed to flush file");

        console_bar.inc(1);
    }

    console_bar.finish_with_message("Finished all transactions.");

    let elapsed = start.elapsed();
    println!(
        "Finished execution. Total CPU time: {:.6}s",
        elapsed.as_secs_f64()
    );

    Ok(())
}
