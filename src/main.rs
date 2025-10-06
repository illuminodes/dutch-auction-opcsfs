fn main() {
    println!("Hello, world!");
}

// Player A
// nsec1nplkcc6tdgm4duh3ehc42kz7syrh7qzr9h9r9q5aqe8x8kuyqr9qtz24g0
// 6214f52a53639da8bb42b7306ddd4b4163a46711cf0f14bda0585715880107f1
// Player B
// nsec1tk2jzxtcex9um4gdz0elcnaejwrkwag5th3d6w3cce4ht7tj7xmsr9ydq8
// 2d8da9cfdb72cdedf2503d711eed06bc21c324b346b2a3b6436d7f7e60cb28dc

#[derive(Debug, thiserror::Error)]
pub enum AuctionError {
    #[error("Failed to decode hex {0}")]
    HexDecode(#[from] hex::FromHexError),
    #[error("Failed to convert integer {0}")]
    IntegerConversion(#[from] std::num::TryFromIntError),
    #[error("secp256k1 error {0}")]
    Secp256k1(#[from] bitcoin::secp256k1::Error),
    #[error("Address error {0}")]
    AddressParse(#[from] bitcoin::address::ParseError),
    #[error("nostr error {0}")]
    Nostr(#[from] nostro2::errors::NostrErrors),
    #[error("Keypair error {0}")]
    Keypair(#[from] nostro2_signer::errors::NostrKeypairError),
    #[error("Taproot error {0}")]
    Taproot(#[from] bitcoin::taproot::TaprootError),
    #[error("Taproot builder error {0}")]
    TaprootBuilder(#[from] bitcoin::taproot::TaprootBuilderError),
    #[error("No ID")]
    NoId,
    #[error("No Pubkey")]
    NoPubkey,
    #[error("No Signature")]
    NoSig,
    #[error("No control block")]
    NoControlBlock,
    #[error("Finalized taproot could not be built")]
    CouldNotBuildBidAddress,
}
pub static BTC_ESPLORA_CLIENT: std::sync::LazyLock<bdk_esplora::esplora_client::AsyncClient> =
    std::sync::LazyLock::new(|| {
        bdk_esplora::esplora_client::Builder::new("https://mutinynet.com/api")
            .build_async()
            .expect("Failed to create BTC Esplora client")
    });

const OP_CHECKSIGFROMSTACK: u8 = 0xcc;
static SECP: std::sync::LazyLock<bitcoin::secp256k1::Secp256k1<bitcoin::secp256k1::All>> =
    std::sync::LazyLock::new(bitcoin::secp256k1::Secp256k1::new);

/// The bidding address is a Taproot address with a control block for each participant.
/// Every participant can claim the funds by providing a valid signed Nostr note as proof of authorization.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BiddingAddress {
    network: bitcoin::Network,
    address: String,
    control_blocks: Vec<(String, bitcoin::taproot::ControlBlock)>,
}

impl BiddingAddress {
    pub fn new(network: bitcoin::Network, participants: &[&str]) -> Result<Self, AuctionError> {
        let mut bid_address = bitcoin::taproot::TaprootBuilder::new();
        for pubkey in participants {
            let template = AcceptTemplate::new(pubkey)?;
            let script = template.accept_script()?;
            bid_address = bid_address.add_leaf(1, script)?;
        }
        let Ok(bid_address) = bid_address.finalize(&SECP, Self::nums_point()?) else {
            return Err(AuctionError::CouldNotBuildBidAddress);
        };
        let mut control_blocks = Vec::new();
        for pubkey in participants {
            let template = AcceptTemplate::new(pubkey)?;
            let script = template.accept_script()?;
            let control_block = bid_address
                .control_block(&(script.clone(), bitcoin::taproot::LeafVersion::TapScript))
                .ok_or(AuctionError::NoControlBlock)?;
            control_blocks.push((template.0.pubkey, control_block));
        }

        let address = bitcoin::Address::p2tr_tweaked(bid_address.output_key(), network);
        Ok(Self {
            network,
            address: address.to_string(),
            control_blocks,
        })
    }
    pub fn checked_address(&self) -> Result<bitcoin::Address, AuctionError> {
        use std::str::FromStr;
        Ok(bitcoin::Address::from_str(&self.address)
            .and_then(|a| a.require_network(self.network))?)
    }

    fn nums_point() -> Result<bitcoin::XOnlyPublicKey, AuctionError> {
        let nums_bytes = [
            0x50, 0x92, 0x9b, 0x74, 0xc1, 0xa0, 0x49, 0x54, 0xb7, 0x8b, 0x4b, 0x60, 0x35, 0xe9,
            0x7a, 0x5e, 0x07, 0x8a, 0x5a, 0x0f, 0x28, 0xec, 0x96, 0xd5, 0x47, 0xbf, 0xee, 0x9a,
            0xce, 0x80, 0x3a, 0xc0,
        ];

        Ok(bitcoin::XOnlyPublicKey::from_slice(&nums_bytes)?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptTemplate(nostro2::NostrNote);

impl AcceptTemplate {
    pub fn new(pubkey: &str) -> Result<Self, AuctionError> {
        let mut note = nostro2::NostrNote {
            content: "Give me my money".to_string(),
            kind: 666,
            pubkey: pubkey.to_string(),
            created_at: 666,
            ..Default::default()
        };
        note.serialize_id()?;
        assert_eq!(note.id.as_ref().map(|s| s.len()), Some(64));
        Ok(Self(note))
    }
    pub fn sign(
        &mut self,
        nostr_keypair: &nostro2_signer::keypair::NostrKeypair,
    ) -> Result<(), AuctionError> {
        nostr_keypair.sign_note(&mut self.0)?;
        Ok(())
    }
    pub fn accept_script(&self) -> Result<bitcoin::ScriptBuf, AuctionError> {
        let bid_accepted_id = hex::decode(self.0.id.as_ref().ok_or(AuctionError::NoId)?)?;
        if self.0.pubkey.is_empty() {
            return Err(AuctionError::NoPubkey);
        }
        let bid_accepted_pubkey = hex::decode(&self.0.pubkey)?;
        let mut script_bytes = Vec::new();
        // push outcome message hash len + bytes(32 bytes)
        script_bytes.push(bid_accepted_id.len().try_into()?);
        script_bytes.extend_from_slice(bid_accepted_id.as_slice());

        // push oracle pubkey len + bytes (32 bytes)
        script_bytes.push(bid_accepted_pubkey.len().try_into()?);
        script_bytes.extend_from_slice(bid_accepted_pubkey.as_slice());

        // push OP_CHECKSIGFROMSTACK
        script_bytes.push(OP_CHECKSIGFROMSTACK);

        Ok(bitcoin::ScriptBuf::from_bytes(script_bytes))
    }
    pub fn witness(
        &self,
        control_block: &bitcoin::taproot::ControlBlock,
    ) -> Result<bitcoin::Witness, AuctionError> {
        let mut wit = bitcoin::Witness::new();
        wit.push(hex::decode(self.0.sig.as_ref().ok_or(AuctionError::NoSig)?)?.as_slice());
        wit.push(hex::decode(self.0.id.as_ref().ok_or(AuctionError::NoId)?)?.as_slice());
        wit.push(self.accept_script()?.to_bytes());
        wit.push(control_block.serialize());

        Ok(wit)
    }
}

#[cfg(test)]
mod tests {

    use crate::BTC_ESPLORA_CLIENT;

    static ALICE_KEYPAIR: std::sync::LazyLock<nostro2_signer::keypair::NostrKeypair> =
        std::sync::LazyLock::new(|| {
            let mut kp = "nsec1nplkcc6tdgm4duh3ehc42kz7syrh7qzr9h9r9q5aqe8x8kuyqr9qtz24g0"
                .parse::<nostro2_signer::keypair::NostrKeypair>()
                .expect("Failed to parse Alice's key");
            kp.set_extractable(true);
            kp
        });

    static ALICE_WALLET: std::sync::LazyLock<tokio::sync::RwLock<bdk_wallet::Wallet>> =
        std::sync::LazyLock::new(|| {
            let xprv: bitcoin::bip32::Xpriv = bitcoin::bip32::Xpriv::new_master(
                bitcoin::Network::Signet,
                ALICE_KEYPAIR
                    .mnemonic(nostro2_signer::Language::English)
                    .expect("Failed to get mnemonic")
                    .as_bytes(),
            )
            .unwrap();
            let (descriptor, _key_map, _) = bdk_wallet::template::DescriptorTemplate::build(
                bdk_wallet::descriptor::template::Bip86(xprv, bdk_wallet::KeychainKind::External),
                bitcoin::Network::Signet,
            )
            .expect("Failed to build external descriptor");

            let (change_descriptor, _change_key_map, _) =
                bdk_wallet::template::DescriptorTemplate::build(
                    bdk_wallet::descriptor::template::Bip86(
                        xprv,
                        bdk_wallet::KeychainKind::Internal,
                    ),
                    bitcoin::Network::Signet,
                )
                .expect("Failed to build internal descriptor");
            let wallet = bdk_wallet::Wallet::create(descriptor, change_descriptor)
                .network(bitcoin::Network::Signet)
                .create_wallet_no_persist()
                .expect("valid wallet");
            tokio::sync::RwLock::new(wallet)
        });

    static BOB_KEYPAIR: std::sync::LazyLock<nostro2_signer::keypair::NostrKeypair> =
        std::sync::LazyLock::new(|| {
            "nsec1tk2jzxtcex9um4gdz0elcnaejwrkwag5th3d6w3cce4ht7tj7xmsr9ydq8"
                .parse::<nostro2_signer::keypair::NostrKeypair>()
                .expect("Failed to parse Bob's key")
        });

    #[test]
    fn alice_and_bob_have_keys() {
        assert_eq!(ALICE_KEYPAIR.public_key().len(), 64);
        assert_eq!(BOB_KEYPAIR.public_key().len(), 64);
        assert_ne!(ALICE_KEYPAIR.public_key(), BOB_KEYPAIR.public_key());
        assert_eq!(
            "6214f52a53639da8bb42b7306ddd4b4163a46711cf0f14bda0585715880107f1",
            ALICE_KEYPAIR.public_key()
        );
        assert_eq!(
            "2d8da9cfdb72cdedf2503d711eed06bc21c324b346b2a3b6436d7f7e60cb28dc",
            BOB_KEYPAIR.public_key()
        );
    }

    #[test]
    fn alice_and_bob_can_sign_notes() {
        let mut note = nostro2::NostrNote {
            content: "test".to_string(),
            pubkey: ALICE_KEYPAIR.public_key().to_string(),
            ..Default::default()
        };
        ALICE_KEYPAIR
            .sign_note(&mut note)
            .expect("Failed to sign note");
        assert_eq!(note.pubkey, ALICE_KEYPAIR.public_key());
        assert_eq!(note.id.as_ref().map(|s| s.len()), Some(64));
        assert_eq!(note.sig.as_ref().map(|s| s.len()), Some(128));
        assert!(note.verify());
        // mutate the sig to ensure it fails if tampered with
        note.sig.replace("tampered".to_string());
        assert!(!note.verify());
        BOB_KEYPAIR
            .sign_note(&mut note)
            .expect("Failed to sign note");
        assert_eq!(note.pubkey, BOB_KEYPAIR.public_key());
        assert_eq!(note.id.as_ref().map(|s| s.len()), Some(64));
        assert_eq!(note.sig.as_ref().map(|s| s.len()), Some(128));
        assert!(note.verify());
    }

    #[test]
    fn create_outcome_scripts_for_alice() {
        let bid_accepted_template =
            super::AcceptTemplate::new(&ALICE_KEYPAIR.public_key().to_string())
                .expect("Failed to create accept template");
        let bid_accepted_template_reconstructed =
            super::AcceptTemplate::new(&ALICE_KEYPAIR.public_key().to_string())
                .expect("Failed to create accept template");
        // ensure you can reconstruct the id from the agreed content
        assert_eq!(bid_accepted_template, bid_accepted_template_reconstructed);
        // a Nostr ID is already a sha256, so we dont need to rehash it

        let script_buf = bid_accepted_template
            .accept_script()
            .expect("Failed to build bid accepted script");
        let script_bytes = script_buf.to_bytes();

        println!("bid accepted script: {}", hex::encode(&script_bytes));
        assert_eq!(script_bytes.len(), 67);
        assert_eq!(script_bytes[0], 32);
        assert_eq!(script_bytes[33], 32);
        assert_eq!(script_bytes[66], super::OP_CHECKSIGFROMSTACK);

        println!("bid accepted script buf: {}", script_buf);
    }

    #[test]
    fn build_bid_address_script() {
        // First we create a bid accepted note for both Alice and Bob
        let bid_address = super::BiddingAddress::new(
            bitcoin::Network::Signet,
            &[
                &ALICE_KEYPAIR.public_key().to_string(),
                &BOB_KEYPAIR.public_key().to_string(),
            ],
        )
        .expect("Failed to create bid address");
        let address = bid_address.address;
        println!("bid address: {address}");
        assert!(address.to_string().starts_with("tb1"));
        assert_eq!(bid_address.control_blocks.len(), 2);
    }
    #[tokio::test]
    async fn alice_can_check_her_wallet() {
        let mut wallet = ALICE_WALLET.write().await;
        let balance = wallet.balance();
        println!("Alice's balance: {balance}");
        let full_scan_request = wallet.start_full_scan();
        let update = bdk_esplora::EsploraAsyncExt::full_scan(
            &*super::BTC_ESPLORA_CLIENT,
            full_scan_request,
            4,
            4,
        )
        .await
        .unwrap();

        // Apply the update from the full scan to the wallet
        wallet.apply_update(update).unwrap();

        let new_balance = wallet.balance();
        println!("Alice's new balance: {new_balance}");
    }
    #[tokio::test]
    async fn bid_address_is_funded() {
        let address = super::BiddingAddress::new(
            bitcoin::Network::Signet,
            &[
                &ALICE_KEYPAIR.public_key().to_string(),
                &BOB_KEYPAIR.public_key().to_string(),
            ],
        )
        .expect("Failed to create bid address");

        let txs = super::BTC_ESPLORA_CLIENT
            .get_address_stats(&address.checked_address().expect("Failed to parse address"))
            .await
            .unwrap();
        println!("txs: {txs:?}");

        assert!(txs.chain_stats.funded_txo_sum > 0);
    }
    #[tokio::test]
    async fn alice_can_spend_bid() {
        let bid_address = super::BiddingAddress::new(
            bitcoin::Network::Signet,
            &[
                &ALICE_KEYPAIR.public_key().to_string(),
                &BOB_KEYPAIR.public_key().to_string(),
            ],
        )
        .expect("Failed to create bid address");

        // 3) Compute the control block for this exact leaf.
        let ctrl = bid_address
            .control_blocks
            .iter()
            .find_map(|(pubkey, blk)| (pubkey == &ALICE_KEYPAIR.public_key()).then_some(blk))
            .expect("failed to find control block");
        println!("control block: {ctrl:?}");

        // Sign the note with Alice's key, will add the signature to the note
        let mut alice_accepted_template =
            super::AcceptTemplate::new(&ALICE_KEYPAIR.public_key().to_string())
                .expect("Failed to create accept template");
        alice_accepted_template
            .sign(&ALICE_KEYPAIR)
            .expect("Failed to sign accept template");
        // Build witness: [<alice_signatue> <message>  <script> <control_block>]
        let wit = alice_accepted_template
            .witness(ctrl)
            .expect("Failed to build witness");

        // print the address
        println!("bid address: {}", bid_address.address);
        let address_utxos = BTC_ESPLORA_CLIENT
            .get_address_txs(
                &bid_address
                    .checked_address()
                    .expect("Failed to parse address"),
                None,
            )
            .await
            .unwrap();

        let mut all_outputs = Vec::new();
        for tx in &address_utxos {
            for (vout_index, vout) in tx.vout.iter().enumerate() {
                if vout.scriptpubkey
                    == bid_address
                        .checked_address()
                        .expect("Failed to parse address")
                        .script_pubkey()
                {
                    all_outputs.push((tx.txid, vout_index as u32, vout.value));
                }
            }
        }
        let mut spent_outpoints = std::collections::HashSet::new();
        for tx in &address_utxos {
            for vin in &tx.vin {
                spent_outpoints.insert((vin.txid, vin.vout));
            }
        }

        let Some(spendable_utxo) = all_outputs.into_iter().find_map(|(txid, vout_index, _)| {
            (!spent_outpoints.contains(&(txid, vout_index))).then_some(bitcoin::OutPoint {
                txid,
                vout: vout_index,
            })
        }) else {
            panic!("No spendable UTXO found");
        };

        // an address to send the money to
        let to_address = ALICE_WALLET
            .write()
            .await
            .next_unused_address(bdk_wallet::KeychainKind::External);

        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: bitcoin::locktime::absolute::LockTime::ZERO,
            input: vec![bitcoin::TxIn {
                sequence: bitcoin::blockdata::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
                previous_output: spendable_utxo,
                witness: wit,
                ..Default::default()
            }],
            output: vec![bitcoin::TxOut {
                // TODO: spend the whole amount
                value: bitcoin::Amount::from_sat(1000),
                script_pubkey: to_address.script_pubkey(),
            }],
        };
        println!("Broadcasting transaction...");
        use bitcoin::consensus::encode;

        let tx_hex = encode::serialize_hex(&tx);
        println!("{:?}", tx_hex);
        BTC_ESPLORA_CLIENT.broadcast(&tx).await.unwrap();
    }
}
