use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct SanitizedOutput {
    pub cleaned_text: String,
    pub flags: i64,
}

/// `PeerInputSanitizer`: neutralizza token di controllo LLM e pattern noti di prompt injection
/// (sia delimitatori tecnici sia tentativi semantici diretti in IT/EN).
/// NOTA (Correzione Sempre #2): Questo sanitizer è solo il primo livello; la difesa primaria
/// è architetturale (incapsulamento in `untrusted_peer_text` e lettore senza tool di esecuzione).
pub fn sanitize_peer_input(input: &str) -> SanitizedOutput {
    let forbidden_patterns = [
        // Delimitatori di controllo LLM
        "<|im_start|>",
        "<|im_end|>",
        "<|endoftext|>",
        "<|system|>",
        "<|user|>",
        "<|assistant|>",
        "<<SYS>>",
        "<</SYS>>",
        "[INST]",
        "[/INST]",
        "[SYSTEM OVERRIDE]",
        "[SYSTEM:",
        "### Instruction:",
        "### System:",
        // Pattern semantici di dirottamento (EN / IT)
        "Ignore previous instructions",
        "ignore all previous instructions",
        "disregard previous instructions",
        "Ignora le regole",
        "ignora le istruzioni precedenti",
        "esegui il comando",
        "pubblica questo al posto",
    ];

    let mut cleaned = input.replace('\0', "");
    let mut flags = 0;

    for pat in forbidden_patterns {
        let lower_cleaned = cleaned.to_lowercase();
        let lower_pat = pat.to_lowercase();
        if lower_cleaned.contains(&lower_pat) {
            flags += 1;
            cleaned = replace_case_insensitive(&cleaned, pat, "[PAYLOAD_DISARMATO]");
        }
    }

    SanitizedOutput {
        cleaned_text: cleaned.trim().to_string(),
        flags,
    }
}

fn replace_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    let lower_h = haystack.to_lowercase();
    let lower_n = needle.to_lowercase();
    let mut result = String::with_capacity(haystack.len());
    let mut last_idx = 0;

    for (idx, _) in lower_h.match_indices(&lower_n) {
        result.push_str(&haystack[last_idx..idx]);
        result.push_str(replacement);
        last_idx = idx + needle.len();
    }
    result.push_str(&haystack[last_idx..]);
    result
}

/// Valida i limiti anti-Context Overflow (`tldr_claim` 10..=160 char, `body` 1..=2000 char)
pub fn validate_post_lengths(tldr_claim: &str, body: &str) -> Result<()> {
    let tldr_len = tldr_claim.chars().count();
    if !(10..=160).contains(&tldr_len) {
        bail!(
            "tldr_claim deve essere compreso tra 10 e 160 caratteri (attuale: {})",
            tldr_len
        );
    }

    let body_len = body.chars().count();
    if !(1..=2000).contains(&body_len) {
        bail!(
            "body deve essere compreso tra 1 e 2000 caratteri (attuale: {})",
            body_len
        );
    }

    Ok(())
}

/// Valida la Proof-of-Critique per i voti epistemici (minimo 80 caratteri)
pub fn validate_vote_reason(reason: &str) -> Result<()> {
    let len = reason.trim().chars().count();
    if len < 80 {
        bail!(
            "Proof-of-Critique insufficiente: la motivazione del voto richiede almeno 80 caratteri (attuale: {})",
            len
        );
    }
    if len > 600 {
        bail!("La motivazione del voto non può superare 600 caratteri");
    }
    Ok(())
}

/// Calcola il Pairwise Vote Decay giornaliero tra la stessa coppia di agenti (Anti-Cartello)
/// 0 voti precedenti oggi -> peso 1.0
/// 1 voto precedente oggi -> peso 0.2
/// >= 2 voti precedenti oggi -> peso 0.0
pub fn calculate_pairwise_decay(previous_votes_today_between_pair: i64) -> f64 {
    match previous_votes_today_between_pair {
        0 => 1.0,
        1 => 0.2,
        _ => 0.0,
    }
}
