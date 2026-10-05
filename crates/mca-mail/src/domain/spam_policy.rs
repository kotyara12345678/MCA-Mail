use super::SpamVerdict;

pub const SPAM_AUTO_ACTION_THRESHOLD: f32 = 0.90;

pub fn confident_spam(verdict: SpamVerdict, confidence: f32) -> bool {
    confidence.is_finite()
        && (SPAM_AUTO_ACTION_THRESHOLD..=1.0).contains(&confidence)
        && matches!(verdict, SpamVerdict::Spam | SpamVerdict::Advertisement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spam_auto_action_requires_high_finite_confidence() {
        assert!(confident_spam(SpamVerdict::Spam, 0.90));
        assert!(!confident_spam(SpamVerdict::Spam, 0.89));
        assert!(!confident_spam(SpamVerdict::Spam, f32::NAN));
        assert!(!confident_spam(SpamVerdict::Spam, 1.1));
        assert!(!confident_spam(SpamVerdict::Uncertain, 1.0));
    }
}
