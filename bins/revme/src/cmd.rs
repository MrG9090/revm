pub mod bench;
pub mod bytecode;
pub mod evmrunner;
pub mod statetest;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(infer_subcommands = true)]
#[allow(clippy::large_enum_variant)]
pub enum MainCmd {
    /// Execute Ethereum state tests.
    Statetest(statetest::Cmd),
    /// Run arbitrary EVM bytecode.
    Evm(evmrunner::Cmd),
    /// Print the structure of an EVM bytecode.
    Bytecode(bytecode::Cmd),
    /// Run bench from specified list.
    Bench(bench::Cmd),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Statetest(#[from] statetest::Error),
    #[error(transparent)]
    EvmRunnerErrors(#[from] evmrunner::Errors),
    #[error("Custom error: {0}")]
    Custom(&'static str),
}

impl MainCmd {
    pub fn run(&self) -> Result<(), Error> {
        match self {
            Self::Statetest(cmd) => cmd.run()?,
            Self::Evm(cmd) => cmd.run()?,
            Self::Bytecode(cmd) => {
                cmd.run();
            }
            Self::Bench(cmd) => {
                cmd.run();
            }
        }
        Ok(())
    }
}
