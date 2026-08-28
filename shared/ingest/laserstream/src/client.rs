//! Generated Yellowstone gRPC client — the tonic half of the `geyser` protobuf.
//!
//! Split out of `ingest-core`'s `generated/geyser.rs` at the seam prost-build
//! already puts there: message types stay in the engine (every decoder signature
//! names `SubscribeUpdateTransaction`), the service client moves here with the
//! rest of the gRPC wire. That split is what lets `ingest-core` carry no tonic.
//!
//! Checked in and hand-maintained — there is no `build.rs` and no `.proto` in the
//! tree — so this is a one-time cut, not a codegen constraint. The generated code
//! reaches its message types through `super::`, which the import below supplies.

#[allow(unused_imports)]
use ingest_core::proto::geyser::*;

/// Generated client implementations.
pub mod geyser_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct GeyserClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl GeyserClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> GeyserClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> GeyserClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
            >>::Error: Into<StdError> + Send + Sync,
        {
            GeyserClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn subscribe(
            &mut self,
            request: impl tonic::IntoStreamingRequest<Message = super::SubscribeRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::SubscribeUpdate>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/geyser.Geyser/Subscribe");
            let mut req = request.into_streaming_request();
            req.extensions_mut().insert(GrpcMethod::new("geyser.Geyser", "Subscribe"));
            self.inner.streaming(req, path, codec).await
        }
        pub async fn ping(
            &mut self,
            request: impl tonic::IntoRequest<super::PingRequest>,
        ) -> std::result::Result<tonic::Response<super::PongResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/geyser.Geyser/Ping");
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new("geyser.Geyser", "Ping"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_latest_blockhash(
            &mut self,
            request: impl tonic::IntoRequest<super::GetLatestBlockhashRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetLatestBlockhashResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/geyser.Geyser/GetLatestBlockhash",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("geyser.Geyser", "GetLatestBlockhash"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_block_height(
            &mut self,
            request: impl tonic::IntoRequest<super::GetBlockHeightRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetBlockHeightResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/geyser.Geyser/GetBlockHeight",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("geyser.Geyser", "GetBlockHeight"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_slot(
            &mut self,
            request: impl tonic::IntoRequest<super::GetSlotRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetSlotResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/geyser.Geyser/GetSlot");
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new("geyser.Geyser", "GetSlot"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn is_blockhash_valid(
            &mut self,
            request: impl tonic::IntoRequest<super::IsBlockhashValidRequest>,
        ) -> std::result::Result<
            tonic::Response<super::IsBlockhashValidResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/geyser.Geyser/IsBlockhashValid",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("geyser.Geyser", "IsBlockhashValid"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_version(
            &mut self,
            request: impl tonic::IntoRequest<super::GetVersionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetVersionResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::new(
                        tonic::Code::Unknown,
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/geyser.Geyser/GetVersion");
            let mut req = request.into_request();
            req.extensions_mut().insert(GrpcMethod::new("geyser.Geyser", "GetVersion"));
            self.inner.unary(req, path, codec).await
        }
    }
}
