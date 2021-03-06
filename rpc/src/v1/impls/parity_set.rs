// Copyright 2015, 2016 Parity Technologies (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

/// Parity-specific rpc interface for operations altering the settings.
use std::io;
use std::sync::{Arc, Weak};

use ethcore::miner::MinerService;
use ethcore::client::MiningBlockChainClient;
use ethcore::mode::Mode;
use ethsync::ManageNetwork;
use fetch::{self, Fetch};
use futures::Future;
use util::sha3;
use updater::{Service as UpdateService};
use parity_reactor::Remote;

use jsonrpc_core::Error;
use jsonrpc_macros::Ready;
use v1::helpers::errors;
use v1::traits::ParitySet;
use v1::types::{Bytes, H160, H256, U256, ReleaseInfo};

/// Parity-specific rpc interface for operations altering the settings.
pub struct ParitySetClient<C, M, U, F=fetch::Client> where
	C: MiningBlockChainClient,
	M: MinerService,
	U: UpdateService,
	F: Fetch,
{
	client: Weak<C>,
	miner: Weak<M>,
	updater: Weak<U>,
	net: Weak<ManageNetwork>,
	fetch: F,
	remote: Remote,
}

impl<C, M, U, F> ParitySetClient<C, M, U, F> where
	C: MiningBlockChainClient,
	M: MinerService,
	U: UpdateService,
	F: Fetch,
{
	/// Creates new `ParitySetClient` with given `Fetch`.
	pub fn new(client: &Arc<C>, miner: &Arc<M>, updater: &Arc<U>, net: &Arc<ManageNetwork>, fetch: F, remote: Remote) -> Self {
		ParitySetClient {
			client: Arc::downgrade(client),
			miner: Arc::downgrade(miner),
			updater: Arc::downgrade(updater),
			net: Arc::downgrade(net),
			fetch: fetch,
			remote: remote,
		}
	}

	fn active(&self) -> Result<(), Error> {
		// TODO: only call every 30s at most.
		take_weak!(self.client).keep_alive();
		Ok(())
	}
}

impl<C, M, U, F> ParitySet for ParitySetClient<C, M, U, F> where
	C: MiningBlockChainClient + 'static,
	M: MinerService + 'static,
	U: UpdateService + 'static,
	F: Fetch + 'static,
{

	fn set_min_gas_price(&self, gas_price: U256) -> Result<bool, Error> {
		self.active()?;

		take_weak!(self.miner).set_minimal_gas_price(gas_price.into());
		Ok(true)
	}

	fn set_gas_floor_target(&self, target: U256) -> Result<bool, Error> {
		self.active()?;

		take_weak!(self.miner).set_gas_floor_target(target.into());
		Ok(true)
	}

	fn set_gas_ceil_target(&self, target: U256) -> Result<bool, Error> {
		self.active()?;

		take_weak!(self.miner).set_gas_ceil_target(target.into());
		Ok(true)
	}

	fn set_extra_data(&self, extra_data: Bytes) -> Result<bool, Error> {
		self.active()?;

		take_weak!(self.miner).set_extra_data(extra_data.to_vec());
		Ok(true)
	}

	fn set_author(&self, author: H160) -> Result<bool, Error> {
		self.active()?;

		take_weak!(self.miner).set_author(author.into());
		Ok(true)
	}

	fn set_engine_signer(&self, address: H160, password: String) -> Result<bool, Error> {
		self.active()?;
		take_weak!(self.miner).set_engine_signer(address.into(), password).map_err(Into::into).map_err(errors::from_password_error)?;
		Ok(true)
	}

	fn set_transactions_limit(&self, limit: usize) -> Result<bool, Error> {
		self.active()?;

		take_weak!(self.miner).set_transactions_limit(limit);
		Ok(true)
	}

	fn set_tx_gas_limit(&self, limit: U256) -> Result<bool, Error> {
		self.active()?;

		take_weak!(self.miner).set_tx_gas_limit(limit.into());
		Ok(true)
	}

	fn add_reserved_peer(&self, peer: String) -> Result<bool, Error> {
		self.active()?;

		match take_weak!(self.net).add_reserved_peer(peer) {
			Ok(()) => Ok(true),
			Err(e) => Err(errors::invalid_params("Peer address", e)),
		}
	}

	fn remove_reserved_peer(&self, peer: String) -> Result<bool, Error> {
		self.active()?;

		match take_weak!(self.net).remove_reserved_peer(peer) {
			Ok(()) => Ok(true),
			Err(e) => Err(errors::invalid_params("Peer address", e)),
		}
	}

	fn drop_non_reserved_peers(&self) -> Result<bool, Error> {
		self.active()?;

		take_weak!(self.net).deny_unreserved_peers();
		Ok(true)
	}

	fn accept_non_reserved_peers(&self) -> Result<bool, Error> {
		self.active()?;

		take_weak!(self.net).accept_unreserved_peers();
		Ok(true)
	}

	fn start_network(&self) -> Result<bool, Error> {
		take_weak!(self.net).start_network();
		Ok(true)
	}

	fn stop_network(&self) -> Result<bool, Error> {
		take_weak!(self.net).stop_network();
		Ok(true)
	}

	fn set_mode(&self, mode: String) -> Result<bool, Error> {
		take_weak!(self.client).set_mode(match mode.as_str() {
			"offline" => Mode::Off,
			"dark" => Mode::Dark(300),
			"passive" => Mode::Passive(300, 3600),
			"active" => Mode::Active,
			e => { return Err(errors::invalid_params("mode", e.to_owned())); },
		});
		Ok(true)
	}

	fn hash_content(&self, ready: Ready<H256>, url: String) {
		let res = self.active();

		match res {
			Err(e) => ready.ready(Err(e)),
			Ok(()) => {
				let task = self.fetch.fetch(&url).then(move |result| {
					let result = result
							.map_err(errors::from_fetch_error)
							.and_then(|response| {
								sha3(&mut io::BufReader::new(response)).map_err(errors::from_fetch_error)
							})
							.map(Into::into);

					// Receive ready and invoke with result.
					ready.ready(result);
					Ok(()) as Result<(), ()>
				});

				self.remote.spawn(task);
			}
		}
	}

	fn upgrade_ready(&self) -> Result<Option<ReleaseInfo>, Error> {
		self.active()?;
		let updater = take_weak!(self.updater);
		Ok(updater.upgrade_ready().map(Into::into))
	}

	fn execute_upgrade(&self) -> Result<bool, Error> {
		self.active()?;
		let updater = take_weak!(self.updater);
		Ok(updater.execute_upgrade())
	}
}
