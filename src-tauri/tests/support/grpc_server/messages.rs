//! Hand-written prost messages for `fixtures/grpc/server/test/v1/*.proto` (no build.rs).

#[derive(Clone, PartialEq, prost::Message)]
pub struct EchoRequest {
    #[prost(string, tag = "1")]
    pub message: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct EchoResponse {
    #[prost(string, tag = "1")]
    pub message: String,
    #[prost(int32, tag = "2")]
    pub index: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct StreamRequest {
    #[prost(string, tag = "1")]
    pub message: String,
    #[prost(int32, tag = "2")]
    pub count: i32,
    #[prost(int32, tag = "3")]
    pub delay_ms: i32,
    #[prost(int32, tag = "4")]
    pub fail_at: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct SumRequest {
    #[prost(int64, tag = "1")]
    pub value: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct SumResponse {
    #[prost(int64, tag = "1")]
    pub sum: i64,
    #[prost(int32, tag = "2")]
    pub count: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct FailRequest {
    #[prost(int32, tag = "1")]
    pub code: i32,
    #[prost(string, tag = "2")]
    pub message: String,
    #[prost(bytes = "vec", tag = "3")]
    pub details: Vec<u8>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct SleepRequest {
    #[prost(int32, tag = "1")]
    pub ms: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct LargeRequest {
    #[prost(int32, tag = "1")]
    pub size: i32,
}
