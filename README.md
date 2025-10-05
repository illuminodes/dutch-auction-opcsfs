# Reverse Dutch Auction (Taproot CSFS POC)

This project is a **proof of concept (PoC)** implementing a game-like **reverse Dutch auction** mechanism on Bitcoin Signet using Taproot and `OP_CHECKSIGFROMSTACK` (CSFS).  

Two players — **Alice** and **Bob** — share a Taproot address with **two spend paths**. Either can claim the funds at any time by providing a valid signed Nostr note as proof of authorization.

---

## Concept Overview

- **Address Structure:**  
  The Taproot output has **two leaves**:
  - One for Alice’s `CSFS` verification script.
  - One for Bob’s `CSFS` verification script.

- **Game Dynamics:**
  - The shared address can be funded gradually (bit by bit).
  - The longer you wait, the more funds accumulate.
  - Waiting increases the risk the **other player** claims the funds first.

---

## Technical Summary

**Script Structure:**
Each spend path enforces:
`<note_id> <player_pubkey> OP_CHECKSIGFROMSTACK`
Where:
- `note_id` is a pre-agreed Nostr event hash (32 bytes) for each player.
- `player_pubkey` is the player’s Nostr public key (x-only, 32 bytes).
- `OP_CHECKSIGFROMSTACK` verifies that the note signature matches.

**Taproot Tree:**
- Path 0: CSFS script for Alice
- Path 1: CSFS script for Bob
- Internal key: NUMS point (no private key)

- **Control Block:**  
Each player can derive the control block from the finalized `TaprootBuilder` for their leaf to prove inclusion.


## 🧾 License

This project is open-source and experimental.  
Use for educational or research purposes only.

