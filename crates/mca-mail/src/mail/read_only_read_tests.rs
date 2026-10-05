use super::*;

#[tokio::test]
async fn read_only_still_fetches_new_mail() {
    let provider = MockMailProvider::with_messages(vec![inbound()]);
    assert_eq!(provider.guard().mode(), MailMode::ReadOnly);
    let messages = provider.fetch_new().await.expect("read is allowed");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].subject, "Need a quote");
    assert_eq!(messages[0].text_body, "please quote 2 pallets");
}
