//! Tencent Cloud speech translation: signature and message mapping, against the examples in
//! the public documentation (tests/fixtures/tencent/README.md).

use sheng_core::{CaptionEvent, UtteranceId};
use sheng_runtime::config::secret::Secret;
use sheng_runtime::provider::ProviderError;
use sheng_runtime::services::tencent::protocol::{caption_events, classify_code, parse};
use sheng_runtime::services::tencent::signing::{
    signature, signed_url, sorted_query, string_to_sign, ConnectionParams, TencentCredentials,
};
use sheng_runtime::services::tencent::ENDPOINT;
use url::Url;

const SIGN_EXAMPLE: &str = include_str!("fixtures/tencent/sign-example.txt");
const HANDSHAKE: &str = include_str!("fixtures/tencent/handshake.json");
const DRAFT: &str = include_str!("fixtures/tencent/draft.json");
const SENTENCE_END: &str = include_str!("fixtures/tencent/sentence-end.json");
const FINAL: &str = include_str!("fixtures/tencent/final.json");
const ERROR_6008: &str = include_str!("fixtures/tencent/error-6008.json");

const DOC_SENTENCE_ID: &str = "0669d099-ffdf-404e-ad91-5eaff18b20df";

/// Parameters of the documented signing example.
fn doc_params() -> ConnectionParams {
    ConnectionParams {
        timestamp: 1_673_408_372,
        expired: 1_673_494_772,
        nonce: 1_673_408_372,
        voice_id: "c64385ee-3e5c-4fc5-bbfd-7c71addb35b0".into(),
        source: "zh".into(),
        target: "en".into(),
    }
}

fn credentials(app_id: &str, secret_id: &str, secret_key: &str) -> TencentCredentials {
    TencentCredentials::new(
        &Secret::new(app_id),
        Secret::new(secret_id),
        Secret::new(secret_key),
    )
    .expect("numeric app id")
}

fn utterance() -> Option<UtteranceId> {
    Some(UtteranceId::Text(DOC_SENTENCE_ID.into()))
}

#[test]
fn string_to_sign_matches_the_documented_example_byte_for_byte() {
    let creds = credentials("1", "*****Qq1zhZMN8dv0******", "unused");
    let endpoint = Url::parse(ENDPOINT).unwrap();

    let text = string_to_sign(&endpoint, "125922***", &sorted_query(&creds, &doc_params()));

    assert_eq!(text, SIGN_EXAMPLE);
}

#[test]
fn signature_is_base64_of_hmac_sha1() {
    // RFC 2202 test cases 1 and 2 (HMAC-SHA1), Base64 of the published digests.
    let key = Secret::new(String::from_utf8(vec![0x0b; 20]).unwrap());
    assert_eq!(signature(&key, "Hi There"), "thcxhlUFcmTii8C2+zeMjvFGvgA=");
    assert_eq!(
        signature(&Secret::new("Jefe"), "what do ya want for nothing?"),
        "7/zfauXrL6LSdBbV8YTfnCWafHk="
    );
}

#[test]
fn signature_of_the_documented_text_with_its_masked_key_is_stable() {
    // The documentation masks its SecretKey, so its printed signature cannot be reproduced;
    // this value was computed independently (Python hmac/hashlib) from the masked strings.
    let key = Secret::new("*****SkqpeHgqmSz*****");

    assert_eq!(
        signature(&key, SIGN_EXAMPLE),
        "Unf+/NEKM26UKjloD9Mr7KGBH8A="
    );
}

#[test]
fn signed_url_appends_the_url_encoded_signature_last() {
    let creds = credentials("1259220000", "AKIDexample", "secret-key-value");
    let endpoint = Url::parse(ENDPOINT).unwrap();
    let params = doc_params();

    let url = signed_url(&endpoint, &creds, &params).unwrap();

    let expected_text = format!(
        "asr.cloud.tencent.com/asr/speech_translate/1259220000?{}",
        sorted_query(&creds, &params)
    );
    let expected_signature = signature(&Secret::new("secret-key-value"), &expected_text);
    let (unsigned, encoded) = url.as_str().rsplit_once("&signature=").unwrap();
    assert_eq!(unsigned, format!("wss://{expected_text}"));
    let decoded = percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .unwrap();
    assert_eq!(decoded, expected_signature);
    assert!(!encoded.contains(['+', '/', '=']));
    assert!(!url.as_str().contains("secret-key-value"));
}

#[test]
fn documented_signature_is_url_encoded_as_documented() {
    let encoded: String = percent_encoding::utf8_percent_encode(
        "G8jDQBRg1JfeBi/YnTjyjekxfDA=",
        percent_encoding::NON_ALPHANUMERIC,
    )
    .to_string();

    assert_eq!(encoded, "G8jDQBRg1JfeBi%2FYnTjyjekxfDA%3D");
}

#[test]
fn every_connection_gets_fresh_identity() {
    let first = ConnectionParams::fresh("zh", "en");
    let second = ConnectionParams::fresh("zh", "en");

    assert_ne!(first.voice_id, second.voice_id);
    assert!(first.expired > first.timestamp);
    assert!(first.nonce > 0 && first.nonce.to_string().len() <= 10);
}

#[test]
fn app_id_must_be_a_plain_number() {
    for bad in ["", "12a4", "125/../x", "1250000000?x=1"] {
        let result = TencentCredentials::new(&Secret::new(bad), Secret::new("a"), Secret::new("b"));
        assert!(result.is_err(), "{bad:?}");
    }
    let debug = format!(
        "{:?}",
        credentials("1250000000", "visible-id", "visible-key")
    );
    assert!(!debug.contains("visible"));
}

#[test]
fn handshake_reply_carries_no_events() {
    let parsed = parse(HANDSHAKE).unwrap();

    assert_eq!(parsed.sentence, None);
    assert!(!parsed.is_final);
}

#[test]
fn unfinished_sentence_becomes_original_and_translation_drafts() {
    let sentence = parse(DRAFT).unwrap().sentence.unwrap();

    assert_eq!(
        caption_events(sentence),
        vec![
            CaptionEvent::OriginalDraft {
                utterance: utterance(),
                text: "实时".into()
            },
            CaptionEvent::TranslationDraft {
                utterance: utterance(),
                text: "Real time".into()
            },
        ]
    );
}

#[test]
fn finished_sentence_becomes_one_confirmed_pair() {
    let sentence = parse(SENTENCE_END).unwrap().sentence.unwrap();

    assert_eq!(
        caption_events(sentence),
        vec![CaptionEvent::ConfirmedPair {
            utterance: utterance(),
            original: "实时语音翻译".into(),
            translation: "Real time voice translation".into(),
        }]
    );
}

#[test]
fn draft_without_translation_yet_only_updates_the_original() {
    let message = r#"{"code":0,"sentence_id":"s1","result":{"source_text":"你好","target_text":"","sentence_end":false}}"#;

    let events = caption_events(parse(message).unwrap().sentence.unwrap());

    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], CaptionEvent::OriginalDraft { .. }));
}

#[test]
fn sentence_id_inside_result_is_accepted() {
    let message = r#"{"code":0,"result":{"sentence_id":"inner","source_text":"a","target_text":"b","sentence_end":true}}"#;

    let sentence = parse(message).unwrap().sentence.unwrap();

    assert_eq!(sentence.utterance, Some(UtteranceId::Text("inner".into())));
}

#[test]
fn final_one_ends_the_session_and_final_two_does_not() {
    assert!(parse(FINAL).unwrap().is_final);
    assert!(!parse(r#"{"code":0,"final":2}"#).unwrap().is_final);
}

#[test]
fn error_codes_are_normalized_without_service_text() {
    assert_eq!(parse(ERROR_6008).unwrap_err(), ProviderError::Provider);
    assert_eq!(classify_code(6001), ProviderError::Configuration);
    assert_eq!(classify_code(6002), ProviderError::Authentication);
    for code in [6000, 6006, 6007, 6010, 6011] {
        assert_eq!(classify_code(code), ProviderError::Provider, "{code}");
    }
}

#[test]
fn account_and_connection_codes_follow_the_step4_answers() {
    // Not activated, resource package used up, account in arrears: the user must act.
    for code in [6003, 6004, 6005] {
        assert_eq!(classify_code(code), ProviderError::Authentication, "{code}");
    }
    for code in [6009, 6012, 6013, 7000] {
        assert_eq!(classify_code(code), ProviderError::Transport, "{code}");
    }
    let session_codes = [
        (6005, "credential_authentication_failed"),
        (7000, "transport_error"),
    ];
    for (code, expected) in session_codes {
        assert_eq!(classify_code(code).speech_session_error().code(), expected);
    }
    assert_eq!(
        classify_code(6006).speech_session_error().code(),
        "provider_error"
    );
}

#[test]
fn malformed_messages_are_provider_errors() {
    for message in ["", "not json", r#"{"code":"x"}"#, "[1]"] {
        assert_eq!(
            parse(message).unwrap_err(),
            ProviderError::Provider,
            "{message:?}"
        );
    }
}
