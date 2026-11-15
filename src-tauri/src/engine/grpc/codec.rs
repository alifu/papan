//! Messages without generated code (plan 08 §5): [`DynamicCodec`] puts
//! `prost_reflect::DynamicMessage`s on the wire for tonic, and [`from_json`]/[`to_json`]
//! are the proto3 JSON mapping the editor and the log use.

use prost::Message as _;
use prost_reflect::{DeserializeOptions, DynamicMessage, MessageDescriptor, SerializeOptions};
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::Status;

use crate::error::AppError;

/// The largest message sent or received.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
/// The largest response message decoded. Smaller than what is sent: a few bytes of
/// packed `int32` decode to many values, so the decoded size is what costs.
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// A message is shown in the log as JSON up to this long.
pub const MAX_SHOWN_JSON_BYTES: usize = 1024 * 1024;
const TRUNCATED_MARK: &str = "... (truncated)";
/// The largest JSON text accepted for one message.
pub const MAX_JSON_BYTES: usize = 1024 * 1024;

/// Encodes any [`DynamicMessage`] and decodes responses as `output`.
#[derive(Debug, Clone)]
pub struct DynamicCodec {
    output: MessageDescriptor,
}

impl DynamicCodec {
    pub fn new(output: MessageDescriptor) -> Self {
        Self { output }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DynamicEncoder;

#[derive(Debug, Clone)]
pub struct DynamicDecoder(MessageDescriptor);

impl Codec for DynamicCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = DynamicEncoder;
    type Decoder = DynamicDecoder;

    fn encoder(&mut self) -> Self::Encoder {
        DynamicEncoder
    }

    fn decoder(&mut self) -> Self::Decoder {
        DynamicDecoder(self.output.clone())
    }
}

impl Encoder for DynamicEncoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        item.encode(dst)
            .map_err(|_| Status::internal("The message could not be encoded."))
    }
}

impl Decoder for DynamicDecoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        let name = self.0.full_name().to_string();
        DynamicMessage::decode(self.0.clone(), src)
            .map(Some)
            .map_err(|_| Status::internal(format!("The response is not a valid `{name}`.")))
    }
}

/// Parses proto3 JSON as `descriptor`. Empty text is the empty message; unknown fields
/// are refused. The error may quote the text: mask it before showing.
pub fn from_json(descriptor: &MessageDescriptor, text: &str) -> Result<DynamicMessage, AppError> {
    let name = descriptor.full_name();
    if text.len() > MAX_JSON_BYTES {
        return Err(AppError::invalid_input("The message is larger than 1 MiB."));
    }
    if text.trim().is_empty() {
        return Ok(DynamicMessage::new(descriptor.clone()));
    }
    let invalid = |source: serde_json::Error| {
        AppError::invalid_input(format!("The message is not a valid `{name}`: {source}."))
    };
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let options = DeserializeOptions::new().deny_unknown_fields(true);
    let message =
        DynamicMessage::deserialize_with_options(descriptor.clone(), &mut deserializer, &options)
            .map_err(invalid)?;
    deserializer.end().map_err(invalid)?;
    if message.encoded_len() > MAX_MESSAGE_BYTES {
        return Err(AppError::invalid_input(
            "The message is larger than 16 MiB.",
        ));
    }
    Ok(message)
}

/// Compact proto3 JSON: camelCase names, 64-bit integers as strings, defaults skipped.
pub fn to_json(message: &DynamicMessage) -> Result<String, AppError> {
    let options = SerializeOptions::new()
        .stringify_64_bit_integers(true)
        .skip_default_fields(true);
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::new(&mut out);
    message
        .serialize_with_options(&mut serializer, &options)
        .map_err(|source| {
            AppError::internal("The message cannot be shown as JSON.")
                .with_detail(source.to_string())
        })?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Stops accepting bytes at `left`, so serializing a huge message stops early.
struct Bounded {
    out: Vec<u8>,
    left: usize,
}

impl std::io::Write for Bounded {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let take = buf.len().min(self.left);
        self.out.extend_from_slice(&buf[..take]);
        self.left -= take;
        if take < buf.len() {
            return Err(std::io::Error::other("limit"));
        }
        Ok(take)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Like [`to_json`], but never builds more than `limit` bytes: the rest is cut and a
/// mark says so.
pub fn to_json_bounded(message: &DynamicMessage, limit: usize) -> Result<String, AppError> {
    let options = SerializeOptions::new()
        .stringify_64_bit_integers(true)
        .skip_default_fields(true);
    let mut writer = Bounded {
        out: Vec::new(),
        left: limit,
    };
    let mut serializer = serde_json::Serializer::new(&mut writer);
    let result = message.serialize_with_options(&mut serializer, &options);
    let cut = writer.left == 0 && result.is_err();
    if let (Err(source), false) = (result, cut) {
        return Err(AppError::internal("The message cannot be shown as JSON.")
            .with_detail(source.to_string()));
    }
    let mut text = String::from_utf8_lossy(&writer.out).into_owned();
    if cut {
        text.push_str(TRUNCATED_MARK);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::grpc::protos::tests::pool_of;

    const PROTO: &str = r#"syntax = "proto3";
package t;
message Inner { int64 big = 1; }
message Sample {
  string user_name = 1;
  int64 count = 2;
  uint64 total = 3;
  repeated Inner items = 4;
  map<string, int32> tags = 5;
  bytes blob = 6;
  Color color = 7;
}
enum Color { COLOR_UNSPECIFIED = 0; RED = 1; }
"#;

    fn sample() -> MessageDescriptor {
        pool_of(&[("t.proto", PROTO)], "t.proto")
            .get_message_by_name("t.Sample")
            .unwrap()
    }

    #[test]
    fn json_round_trips_through_the_wire_format() {
        let text = r#"{"userName":"ann","count":"9007199254740993","total":"18446744073709551615","items":[{"big":"5"}],"tags":{"a":1},"blob":"AAEC","color":"RED"}"#;

        let message = from_json(&sample(), text).unwrap();
        let bytes = message.encode_to_vec();
        let decoded = DynamicMessage::decode(sample(), bytes.as_slice()).unwrap();

        assert_eq!(to_json(&decoded).unwrap(), text);
    }

    #[test]
    fn proto_names_and_numbers_are_accepted_and_defaults_are_skipped() {
        let message = from_json(&sample(), r#"{"user_name":"","count":7,"color":1}"#).unwrap();

        assert_eq!(to_json(&message).unwrap(), r#"{"count":"7","color":"RED"}"#);
        assert_eq!(to_json(&from_json(&sample(), "  ").unwrap()).unwrap(), "{}");
    }

    #[test]
    fn an_unknown_field_is_refused_with_its_name_and_place() {
        let text = "{\n  \"items\": [{\"nope\": 1}]\n}";

        let error = from_json(&sample(), text).unwrap_err();

        assert!(error.message.contains("`t.Sample`"), "{error}");
        assert!(error.message.contains("nope"), "{error}");
        assert!(error.message.contains("line 2"), "{error}");
    }

    #[test]
    fn trailing_text_wrong_types_and_oversized_json_are_refused() {
        assert!(from_json(&sample(), "{} x").is_err());
        assert!(from_json(&sample(), r#"{"count":"many"}"#).is_err());
        let huge = format!(r#"{{"userName":"{}"}}"#, "a".repeat(MAX_JSON_BYTES));
        let error = from_json(&sample(), &huge).unwrap_err();
        assert!(error.message.contains("1 MiB"));
    }

    #[test]
    fn the_decoder_reports_bytes_that_are_not_the_output_type() {
        let mut codec = DynamicCodec::new(sample());
        let _encoder = codec.encoder();
        let DynamicDecoder(descriptor) = codec.decoder();

        assert_eq!(descriptor.full_name(), "t.Sample");
        assert!(DynamicMessage::decode(descriptor, [0xff_u8, 0xff].as_slice()).is_err());
    }

    #[test]
    fn a_packed_repeated_int32_is_cut_at_the_shown_limit() {
        let pool = pool_of(
            &[(
                "p.proto",
                "syntax = \"proto3\"; package p; message M { repeated int32 v = 1; }",
            )],
            "p.proto",
        );
        let descriptor = pool.get_message_by_name("p.M").unwrap();
        let wire = {
            let mut bytes = vec![0x0a];
            let count = MAX_RESPONSE_BYTES / 2;
            prost::encoding::encode_varint(count as u64, &mut bytes);
            bytes.extend(std::iter::repeat_n(1u8, count));
            bytes
        };
        let message = DynamicMessage::decode(descriptor, wire.as_slice()).unwrap();

        let shown = to_json_bounded(&message, MAX_SHOWN_JSON_BYTES).unwrap();

        assert!(shown.len() <= MAX_SHOWN_JSON_BYTES + TRUNCATED_MARK.len());
        assert!(shown.ends_with(TRUNCATED_MARK));
        let small = DynamicMessage::new(sample());
        assert_eq!(to_json_bounded(&small, 100).unwrap(), "{}");
    }
}
