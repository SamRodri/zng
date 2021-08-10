//! HTTP client.
//!
//! This module is a thin wrapper around the [`isahc`] crate that just that just limits the API
//! surface to only `async` methods. You can convert from/into that [`isahc`] types and this one.
//!
//! # Examples
//!
//! Get some text:
//!
//! ```
//! # use zero_ui_core::task;
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let text = task::http::get_text("https://httpbin.org/base64/SGVsbG8gV29ybGQ=").await?;
//! println!("{}!", text);
//! # Ok(()) }
//! ```
//!
//! [`isahc`]: https://docs.rs/isahc

use std::convert::TryFrom;
use std::sync::Arc;
use std::time::Duration;
use std::{fmt, mem};

use isahc::config::Configurable;
pub use isahc::config::RedirectPolicy;
pub use isahc::cookies::{Cookie, CookieJar};
pub use isahc::error::{Error, ErrorKind};
pub use isahc::http::{header, uri, Method, StatusCode, Uri};

use async_trait::*;
use isahc::{AsyncReadResponseExt, ResponseExt};
use parking_lot::{const_mutex, Mutex};

use crate::crate_util::PanicPayload;
use crate::units::*;

use super::channel;

/// Marker trait for types that try-to-convert to [`Uri`].
///
/// All types `T` that match `Uri: TryFrom<T>, <Uri as TryFrom<T>>::Error: Into<isahc::http::Error>` implement this trait.
pub trait TryUri {
    /// Tries to convert `self` into [`Uri`].
    fn try_into(self) -> Result<Uri, Error>;
}
impl<U> TryUri for U
where
    Uri: TryFrom<U>,
    <Uri as TryFrom<U>>::Error: Into<isahc::http::Error>,
{
    fn try_into(self) -> Result<Uri, Error> {
        Uri::try_from(self).map_err(|e| e.into().into())
    }
}

/// Marker trait for types that try-to-convert to [`Method`].
///
/// All types `T` that match `Method: TryFrom<T>, <Method as TryFrom<T>>::Error: Into<isahc::http::Error>` implement this trait.
pub trait TryMethod {
    /// Tries to convert `self` into [`Method`].
    fn try_into(self) -> Result<Method, Error>;
}
impl<U> TryMethod for U
where
    Method: TryFrom<U>,
    <isahc::http::Method as TryFrom<U>>::Error: Into<isahc::http::Error>,
{
    fn try_into(self) -> Result<Method, Error> {
        Method::try_from(self).map_err(|e| e.into().into())
    }
}

/// Marker trait for types that try-to-convert to [`Body`].
///
/// All types `T` that match `isahc::AsyncBody: TryFrom<T>, <isahc::AsyncBody as TryFrom<T>>::Error: Into<isahc::http::Error>`
/// implement this trait.
pub trait TryBody {
    /// Tries to convert `self` into [`Body`].
    fn try_into(self) -> Result<Body, Error>;
}
impl<U> TryBody for U
where
    isahc::AsyncBody: TryFrom<U>,
    <isahc::AsyncBody as TryFrom<U>>::Error: Into<isahc::http::Error>,
{
    fn try_into(self) -> Result<Body, Error> {
        match isahc::AsyncBody::try_from(self) {
            Ok(r) => Ok(Body(r)),
            Err(e) => Err(e.into().into()),
        }
    }
}

/// Marker trait for types that try-to-convert to [`header::HeaderName`].
///
/// All types `T` that match `header::HeaderName: TryFrom<T>, <header::HeaderName as TryFrom<T>>::Error: Into<isahc::http::Error>`
/// implement this trait.
pub trait TryHeaderName {
    /// Tries to convert `self` into [`Body`].
    fn try_into(self) -> Result<header::HeaderName, Error>;
}
impl<U> TryHeaderName for U
where
    header::HeaderName: TryFrom<U>,
    <header::HeaderName as TryFrom<U>>::Error: Into<isahc::http::Error>,
{
    fn try_into(self) -> Result<header::HeaderName, Error> {
        header::HeaderName::try_from(self).map_err(|e| e.into().into())
    }
}

/// Marker trait for types that try-to-convert to [`header::HeaderValue`].
///
/// All types `T` that match `header::HeaderValue: TryFrom<T>, <header::HeaderValue as TryFrom<T>>::Error: Into<isahc::http::Error>`
/// implement this trait.
pub trait TryHeaderValue {
    /// Tries to convert `self` into [`Body`].
    fn try_into(self) -> Result<header::HeaderValue, Error>;
}
impl<U> TryHeaderValue for U
where
    header::HeaderValue: TryFrom<U>,
    <header::HeaderValue as TryFrom<U>>::Error: Into<isahc::http::Error>,
{
    fn try_into(self) -> Result<header::HeaderValue, Error> {
        header::HeaderValue::try_from(self).map_err(|e| e.into().into())
    }
}

/// HTTP request.
///
/// Use [`send`] to send a request.
#[derive(Debug)]
pub struct Request(isahc::Request<Body>);
impl Request {
    /// Starts an empty builder.
    ///
    /// # Examples
    ///
    /// ```
    /// use zero_ui_core::task::http;
    ///
    /// # fn try_example() -> Result<(), Box<dyn std::error::Error>> {
    /// let request = http::Request::builder().method(http::Method::PUT)?.uri("https://httpbin.org/put")?.build();
    /// # Ok(()) }
    /// ```
    ///
    /// Call [`build`] or [`body`] to finish building the request, note that there are is also an associated function
    /// to start a builder for each HTTP method and uri.
    ///
    /// [`build`]: RequestBuilder::build
    /// [`body`]: RequestBuilder::body
    pub fn builder() -> RequestBuilder {
        RequestBuilder(isahc::Request::builder())
    }

    /// Starts building a GET request.
    ///
    /// # Examples
    ///
    /// ```
    /// use zero_ui_core::task::http;
    ///
    /// # fn try_example() -> Result<(), Box<dyn std::error::Error>> {
    /// let get = http::Request::get("https://httpbin.org/get")?.build();
    /// # Ok(()) }
    /// ```
    pub fn get(uri: impl TryUri) -> Result<RequestBuilder, Error> {
        Ok(RequestBuilder(isahc::Request::get(uri.try_into()?)))
    }

    /// Starts building a PUT request.
    ///
    /// # Examples
    ///
    /// ```
    /// use zero_ui_core::task::http;
    ///
    /// # fn try_example() -> Result<(), Box<dyn std::error::Error>> {
    /// let put = http::Request::put("https://httpbin.org/put")?.header("accept", "application/json")?.build();
    /// # Ok(()) }
    /// ```
    pub fn put(uri: impl TryUri) -> Result<RequestBuilder, Error> {
        Ok(RequestBuilder(isahc::Request::put(uri.try_into()?)))
    }

    /// Starts building a POST request.
    ///
    /// # Examples
    ///
    /// ```
    /// use zero_ui_core::task::http;
    ///
    /// # fn try_example() -> Result<(), Box<dyn std::error::Error>> {
    /// let post = http::Request::post("https://httpbin.org/post")?.header("accept", "application/json")?.build();
    /// # Ok(()) }
    /// ```
    pub fn post(uri: impl TryUri) -> Result<RequestBuilder, Error> {
        Ok(RequestBuilder(isahc::Request::post(uri.try_into()?)))
    }

    /// Starts building a DELETE request.
    ///
    /// # Examples
    ///
    /// ```
    /// use zero_ui_core::task::http;
    ///
    /// # fn try_example() -> Result<(), Box<dyn std::error::Error>> {
    /// let delete = http::Request::delete("https://httpbin.org/delete")?.header("accept", "application/json")?.build();
    /// # Ok(()) }
    /// ```
    pub fn delete(uri: impl TryUri) -> Result<RequestBuilder, Error> {
        Ok(RequestBuilder(isahc::Request::delete(uri.try_into()?)))
    }

    /// Starts building a PATCH request.
    ///
    /// # Examples
    ///
    /// ```
    /// use zero_ui_core::task::http;
    ///
    /// # fn try_example() -> Result<(), Box<dyn std::error::Error>> {
    /// let patch = http::Request::patch("https://httpbin.org/patch")?.header("accept", "application/json")?.build();
    /// # Ok(()) }
    /// ```
    pub fn patch(uri: impl TryUri) -> Result<RequestBuilder, Error> {
        Ok(RequestBuilder(isahc::Request::patch(uri.try_into()?)))
    }

    /// Starts building a HEAD request.
    ///
    /// # Examples
    ///
    /// ```
    /// use zero_ui_core::task::http;
    ///
    /// # fn try_example() -> Result<(), Box<dyn std::error::Error>> {
    /// let head = http::Request::head("https://httpbin.org")?.build();
    /// # Ok(()) }
    /// ```
    pub fn head(uri: impl TryUri) -> Result<RequestBuilder, Error> {
        Ok(RequestBuilder(isahc::Request::head(uri.try_into()?)))
    }
}

/// A [`Request`] builder.
///
/// You can use [`Request::builder`] to start an empty builder.
#[derive(Debug)]
pub struct RequestBuilder(isahc::http::request::Builder);
impl RequestBuilder {
    /// Set the HTTP method for this request.
    pub fn method(self, method: impl TryMethod) -> Result<Self, Error> {
        Ok(Self(self.0.method(method.try_into()?)))
    }

    /// Set the URI for this request.
    pub fn uri(self, uri: impl TryUri) -> Result<Self, Error> {
        Ok(Self(self.0.uri(uri.try_into()?)))
    }

    /// Appends a header to this request.
    pub fn header(self, name: impl TryHeaderName, value: impl TryHeaderValue) -> Result<Self, Error> {
        Ok(Self(self.0.header(name.try_into()?, value.try_into()?)))
    }

    /// Set a cookie jar to use to accept, store, and supply cookies for incoming responses and outgoing requests.
    ///
    /// Note that the default [`isahc_client`] already has a cookie jar.
    pub fn cookie_jar(self, cookie_jar: CookieJar) -> Self {
        Self(self.0.cookie_jar(cookie_jar))
    }

    /// Specify a maximum amount of time that a complete request/response cycle is allowed to
    /// take before being aborted. This includes DNS resolution, connecting to the server,
    /// writing the request, and reading the response.
    ///
    /// Note that this includes the response read operation, so if you get a response but don't
    /// read-it within this timeout you will get a [`TimedOut`] IO error.
    ///
    /// By default no timeout is used.
    ///
    /// [`TimedOut`]: https://doc.rust-lang.org/nightly/std/io/enum.ErrorKind.html#variant.TimedOut
    pub fn timeout(self, timeout: Duration) -> Self {
        Self(self.0.timeout(timeout))
    }

    /// Set a timeout for establishing connections to a host.
    ///
    /// If not set, the [`isahc_client`] default of 90 seconds will be used.
    pub fn connect_timeout(self, timeout: Duration) -> Self {
        Self(self.0.connect_timeout(timeout))
    }

    /// Specify a maximum amount of time where transfer rate can go below a minimum speed limit.
    ///
    /// The `low_speed` limit is in bytes/s. No low-speed limit is configured by default.
    pub fn low_speed_timeout(self, low_speed: u32, timeout: Duration) -> Self {
        Self(self.0.low_speed_timeout(low_speed, timeout))
    }

    /// Set a policy for automatically following server redirects.
    ///
    /// If enabled the "Referer" header will be set automatically too.
    ///
    /// The default [`isahc_client`] follows up-to 20 redirects.
    pub fn redirect_policy(self, policy: RedirectPolicy) -> Self {
        Self(self.0.redirect_policy(policy))
    }

    /// Enable or disable automatic decompression of the response body.
    ///
    /// If enabled the "Accept-Encoding" will also be set automatically, if it was not set using [`header`].
    ///
    /// This is enabled by default.
    ///
    /// [`header`]: Self::header
    pub fn auto_decompress(self, enabled: bool) -> Self {
        Self(self.0.automatic_decompression(enabled))
    }

    /// Set a maximum upload speed for the request body, in bytes per second.
    pub fn max_upload_speed(self, max: u64) -> Self {
        Self(self.0.max_upload_speed(max))
    }

    /// Set a maximum download speed for the response body, in bytes per second.
    pub fn max_download_speed(self, max: u64) -> Self {
        Self(self.0.max_download_speed(max))
    }

    /// Enable or disable metrics collecting.
    ///
    /// When enabled you can get the information using the [`Response::metrics`] method.
    ///
    /// This is enabled by default.
    pub fn metrics(self, enable: bool) -> Self {
        Self(self.0.metrics(enable))
    }

    /// Build the request without a body.
    pub fn build(self) -> Request {
        self.body(()).unwrap()
    }

    /// Build the request with a body.
    pub fn body(self, body: impl TryBody) -> Result<Request, Error> {
        Ok(Request(self.0.body(body.try_into()?).unwrap()))
    }

    /// Build the request with more custom build calls in the [inner builder].
    ///
    /// [inner builder]: isahc::http::request::Builder
    pub fn build_custom<F>(self, custom: F) -> Result<Request, Error>
    where
        F: FnOnce(isahc::http::request::Builder) -> isahc::http::Result<isahc::Request<isahc::AsyncBody>>,
    {
        let req = custom(self.0)?;
        Ok(Request(req.map(Body)))
    }
}

/// HTTP response.
#[derive(Debug)]
pub struct Response(isahc::Response<isahc::AsyncBody>);
impl Response {
    /// Returns the [`StatusCode`].
    #[inline]
    pub fn status(&self) -> StatusCode {
        self.0.status()
    }

    /// Returns a reference to the associated header field map.
    #[inline]
    pub fn headers(&self) -> &header::HeaderMap<header::HeaderValue> {
        self.0.headers()
    }

    /// Get the configured cookie jar used for persisting cookies from this response, if any.
    ///
    /// Only returns `None` if the default [`isahc_client`] was replaced by one with cookies disabled.
    pub fn cookie_jar(&self) -> Option<&CookieJar> {
        self.0.cookie_jar()
    }

    /// Read the response body as a string.
    pub async fn text(&mut self) -> std::io::Result<String> {
        self.0.text().await
    }

    /// Get the effective URI of this response. This value differs from the
    /// original URI provided when making the request if at least one redirect
    /// was followed.
    pub fn effective_uri(&self) -> Option<&Uri> {
        self.0.effective_uri()
    }

    /// Read the response body as raw bytes.
    ///
    /// Use [`DownloadTask`] to get larger files.
    pub async fn bytes(&mut self) -> std::io::Result<Vec<u8>> {
        let cap = self.0.body_mut().len().unwrap_or(1024);
        let mut bytes = Vec::with_capacity(cap as usize);
        self.0.copy_to(&mut bytes).await?;
        Ok(bytes)
    }

    /// Read some bytes from the body, returns how many bytes where read.
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use futures_lite::io::AsyncReadExt;
        self.0.body_mut().read(buf).await
    }

    /// Read the `buf.len()` from the body.
    pub async fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        use futures_lite::io::AsyncReadExt;
        self.0.body_mut().read_exact(buf).await
    }

    /// Deserialize the response body as JSON.
    pub async fn json<O>(&mut self) -> Result<O, serde_json::Error>
    where
        O: serde::de::DeserializeOwned + std::marker::Unpin,
    {
        self.0.json().await
    }

    /// Metrics for the task transfer.
    ///
    /// Metrics are enabled in the default in the [`isahc_client`] and can be toggled for each request using the
    /// [`RequestBuilder::metrics`] method. If disabled returns [`Metrics::zero`].
    pub fn metrics(&self) -> Metrics {
        self.0.metrics().map(Metrics::from_isahc).unwrap_or_else(Metrics::zero)
    }

    /// Drop the request without dropping the connection.
    ///
    /// This receives and discards any remaining bytes in the response stream. When a response
    /// is dropped without finishing the connection is discarded so it cannot be reused for connections
    /// older then HTTP/2.
    ///
    /// You should call this method before dropping if you expect the remaining bytes to be consumed quickly and
    /// don't known that HTTP/2 or newer is being used.
    pub async fn consume(&mut self) -> std::io::Result<()> {
        self.0.consume().await
    }
}
impl From<Response> for isahc::Response<isahc::AsyncBody> {
    fn from(r: Response) -> Self {
        r.0
    }
}

/// HTTP request body.
///
/// Use [`TryBody`] to convert types to body.
#[derive(Debug)]
pub struct Body(isahc::AsyncBody);
impl From<Body> for isahc::AsyncBody {
    fn from(r: Body) -> Self {
        r.0
    }
}

/// Send a GET request to the `uri`.
#[inline]
pub async fn get(uri: impl TryUri) -> Result<Response, Error> {
    isahc_client().get_async(uri.try_into()?).await.map(Response)
}

/// Send a GET request to the `uri` and read the response as a string.
pub async fn get_text(uri: impl TryUri) -> Result<String, Error> {
    let mut r = get(uri).await?;
    let r = r.text().await?;
    Ok(r)
}

/// Send a GET request to the `uri` and read the response as raw bytes.
pub async fn get_bytes(uri: impl TryUri) -> Result<Vec<u8>, Error> {
    let mut r = get(uri).await?;
    let r = r.bytes().await?;
    Ok(r)
}

/// Like [`get_bytes`] but checks a local disk cache first. TODO
pub async fn get_bytes_cached(uri: impl TryUri) -> Result<Vec<u8>, Error> {
    log::warn!("get_bytes_cached is not implemented TODO");
    get_bytes(uri).await
}

/// Send a GET request to the `uri` and de-serializes the response.
pub async fn get_json<O>(uri: impl TryUri) -> Result<O, Box<dyn std::error::Error>>
where
    O: serde::de::DeserializeOwned + std::marker::Unpin,
{
    let mut r = get(uri).await?;
    let r = r.json::<O>().await?;
    Ok(r)
}

/// Send a HEAD request to the `uri`.
#[inline]
pub async fn head(uri: impl TryUri) -> Result<Response, Error> {
    isahc_client().head_async(uri.try_into()?).await.map(Response)
}

/// Send a PUT request to the `uri` with a given request body.
#[inline]
pub async fn put(uri: impl TryUri, body: impl Into<Body>) -> Result<Response, Error> {
    isahc_client().put_async(uri.try_into()?, body.into().0).await.map(Response)
}

/// Send a POST request to the `uri` with a given request body.
#[inline]
pub async fn post(uri: impl TryUri, body: impl Into<Body>) -> Result<Response, Error> {
    isahc_client().post_async(uri.try_into()?, body.into().0).await.map(Response)
}

/// Send a DELETE request to the `uri`.
#[inline]
pub async fn delete(uri: impl TryUri) -> Result<Response, Error> {
    isahc_client().delete_async(uri.try_into()?).await.map(Response)
}

/// Send a custom [`Request`].
#[inline]
pub async fn send(request: Request) -> Result<Response, Error> {
    isahc_client().send_async(request.0).await.map(Response)
}

/// The [`isahc`] client used by the functions in this module and Zero-Ui.
///
/// You can replace the default client at the start of the process using [`set_isahc_client_init`].
///
/// # Defaults
///
/// This the enables `redirect_policy` with a limit of up-to 20 redirects and `auto_referer`, also reduces
/// the `connect_timeout` to 90 seconds and enables `metrics`.
///
/// [`isahc`]: https://docs.rs/isahc
pub fn isahc_client() -> &'static isahc::HttpClient {
    use crate::units::*;
    use once_cell::sync::Lazy;

    static SHARED: Lazy<isahc::HttpClient> = Lazy::new(|| {
        let ci = mem::replace(&mut *CLIENT_INIT.lock(), ClientInit::Inited);
        if let ClientInit::Set(init) = ci {
            init()
        } else {
            // browser defaults
            isahc::HttpClient::builder()
                .cookies()
                .redirect_policy(RedirectPolicy::Limit(20))
                .connect_timeout(90.secs())
                .auto_referer()
                .metrics(true)
                .build()
                .unwrap()
        }
    });
    &SHARED
}

static CLIENT_INIT: Mutex<ClientInit> = const_mutex(ClientInit::None);

enum ClientInit {
    None,
    Set(Box<dyn FnOnce() -> isahc::HttpClient + Send>),
    Inited,
}

/// Set a custom initialization function for the [`isahc_client`].
///
/// The [`isahc_client`] is used by all Zero-Ui functions and is initialized on the first usage,
/// you can use this function before any HTTP operation to replace the [`isahc`] client
/// used by Zero-Ui.
///
/// Returns an error if the [`isahc_client`] was already initialized.
///
/// [`isahc`]: https://docs.rs/isahc
pub fn set_isahc_client_init<I>(init: I) -> Result<(), I>
where
    I: FnOnce() -> isahc::HttpClient + Send + 'static,
{
    let mut ci = CLIENT_INIT.lock();
    if let ClientInit::Inited = &*ci {
        Err(init)
    } else {
        *ci = ClientInit::Set(Box::new(init));
        Ok(())
    }
}

enum DtError {
    Io(std::io::Error),
    Panic(PanicPayload),
}

/// Represents a running large file download.
pub struct DownloadTask {
    receiver: channel::Receiver<Result<Vec<u8>, DtError>>,
    stop_recv: channel::Receiver<Response>,
    payload_len: ByteLength,
}
impl DownloadTask {
    /// Start building a download task using the [default client].
    ///
    /// [default client]: isahc_client
    #[inline]
    pub fn default() -> DownloadTaskBuilder {
        DownloadTaskBuilder::default()
    }

    /// Start building a download task with a custom [`isahc`] client.
    ///
    /// [`isahc`]: https://docs.rs/isahc
    #[inline]
    pub fn with_client(client: isahc::HttpClient) -> DownloadTaskBuilder {
        DownloadTaskBuilder::new(client)
    }

    fn spawn(builder: DownloadTaskBuilder, mut response: Response) -> Self {
        let payload_len = builder.payload_len;

        let (sender, receiver) = channel::bounded(builder.channel_capacity);
        let (stop_sender, stop_recv) = channel::bounded(1);
        let panic_sender = sender.clone();

        let worker = super::run_catch(async move {
            loop {
                let mut buf = vec![0; payload_len.0];
                match response.read(&mut buf).await {
                    Ok(l) => {
                        if l < payload_len.0 {
                            buf.truncate(l);

                            let _ = sender.send(Ok(buf)).await;
                            let _ = stop_sender.send(response).await;
                            return; // cause: EOF
                        } else if sender.send(Ok(buf)).await.is_err() {
                            let _ = stop_sender.send(response).await;
                            return; // cause: receiver dropped
                        }
                    }
                    Err(e) => {
                        debug_assert!(e.kind() != std::io::ErrorKind::Interrupted);
                        let _ = sender.send(Err(DtError::Io(e))).await;
                        let _ = stop_sender.send(response).await;
                        return; // cause: error
                    }
                }
            }
        });
        super::spawn(async move {
            if let Err(panic) = worker.await {
                let _ = panic_sender.send(Err(DtError::Panic(panic))).await;
            }
        });

        Self {
            receiver,
            stop_recv,
            payload_len,
        }
    }

    /// Maximum number of bytes per payload.
    #[inline]
    pub fn payload_len(&self) -> ByteLength {
        self.payload_len
    }

    /// Pause the download.
    ///
    /// This signals the task stop downloading even if there is space in the cache, if you
    /// set `cancel_partial_payloads` any partially downloaded payload is dropped.
    ///
    /// Note that the task naturally *pauses* when the cache limit is reached if you stop calling [`download`],
    /// in this case you do not need to call `pause` or [`resume`].
    ///
    /// [`download`]: Self::download
    /// [`resume`]: Self::resume
    pub async fn pause(&self, cancel_partial_payloads: bool) {
        todo!("{}", cancel_partial_payloads)
    }

    /// Resume the download, if the connection was lost attempts to reconnect.
    pub async fn resume(&self) {
        todo!()
    }

    /// Stops the download but retains the disk cache and returns a [`FrozenDownloadTask`]
    /// that can be serialized/desterilized and resumed.
    pub async fn freeze(self) -> FrozenDownloadTask {
        todo!()
    }

    /// Receive the next downloaded payload.
    ///
    /// The payloads are sequential, even if parallel downloads are enabled.
    pub async fn download(&self) -> Result<Vec<u8>, DownloadTaskError> {
        self.receiver
            .recv()
            .await
            .map_err(|_| DownloadTaskError::Closed)?
            .map_err(|e| match e {
                DtError::Io(e) => e.into(),
                DtError::Panic(p) => std::panic::resume_unwind(p),
            })
    }

    /// Stops the task, cancels download if it is not finished, clears the disk cache if any was used.
    ///
    /// Returns the last [`Response`] used.
    pub async fn stop(self) -> Response {
        drop(self.receiver);
        self.stop_recv
            .recv()
            .await
            .expect("already stoped due to panic in `DownloadTask::download`")
    }
}
#[async_trait]
impl super::ReceiverTask for DownloadTask {
    type Error = DownloadTaskError;

    async fn recv(&self) -> Result<Vec<u8>, Self::Error> {
        self.download().await
    }

    async fn stop(self) {
        let _ = self.stop().await;
    }
}

/// Builds [`DownloadTask`].
///
/// Use [`DownloadTask::default`] or [`DownloadTask::with_client`] to start.
#[derive(Clone)]
pub struct DownloadTaskBuilder {
    client: isahc::HttpClient,
    payload_len: ByteLength,
    channel_capacity: usize,
    parallel_count: usize,
    disk_cache_capacity: usize,
    max_speed: usize,
    request_config: Arc<dyn Fn(RequestBuilder) -> Result<RequestBuilder, Error> + Send>,
}
impl fmt::Debug for DownloadTaskBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DownloadTaskBuilder")
            .field("client", &self.client)
            .field("payload_len", &self.payload_len)
            .field("channel_capacity", &self.channel_capacity)
            .field("parallel_count", &self.parallel_count)
            .field("disk_cache_capacity", &self.disk_cache_capacity)
            .field("max_speed", &self.max_speed)
            .finish_non_exhaustive()
    }
}
impl Default for DownloadTaskBuilder {
    fn default() -> Self {
        Self::new(isahc_client().clone())
    }
}
impl DownloadTaskBuilder {
    fn new(client: isahc::HttpClient) -> Self {
        DownloadTaskBuilder {
            client,
            payload_len: 1.mebi_bytes(),
            channel_capacity: 8,
            parallel_count: 1,
            disk_cache_capacity: 0,
            max_speed: 0,
            request_config: Arc::new(Ok),
        }
    }

    /// Set the number of bytes in each payload.
    ///
    /// Default is one mebibyte (`1024 * 1024`).
    pub fn payload_len(mut self, len: ByteLength) -> Self {
        self.payload_len = len;
        self
    }

    /// Set the number of downloaded payloads that can wait in memory. If this
    /// capacity is reached the disk cache is used if it is set, otherwise the download *pauses*
    /// internally until a payload is taken from the channel.
    ///
    /// Default is `8`.
    pub fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity;
        self
    }

    /// Set the number of payloads that can be downloaded in parallel, setting
    /// this to more than one can speedup the overall download time, if you are
    /// just downloading to a file and depending on the server.
    ///
    /// Default is `1`.
    pub fn parallel_count(mut self, count: usize) -> Self {
        self.parallel_count = count;
        self
    }

    /// Set the number of payloads that can be cached in disk. If this capacity is
    /// reached the download *pauses* and *resumes* internally.
    ///
    /// Default is `0`.
    pub fn disk_cache_capacity(mut self, payload_count: usize) -> Self {
        self.disk_cache_capacity = payload_count;
        self
    }

    /// Set the maximum download speed, in bytes per second.
    ///
    /// Default is `usize::MAX` to indicate no limit. Minimal value is `57344` (56 kibibytes/s).
    #[inline]
    pub fn max_speed(mut self, bytes_per_sec: usize) -> Self {
        self.max_speed = bytes_per_sec;
        self
    }

    /// Set a closure that configures requests generated by the download task.
    ///
    /// # Examples
    ///
    /// Set a custom header:
    ///
    /// ```
    /// # use zero_ui_core::task::http::*;
    /// # fn demo(builder: DownloadTaskBuilder) -> DownloadTaskBuilder {
    /// builder.request_config(|c| c.header("X-Foo-For", "Bar"))
    /// # }
    /// ```
    ///
    /// The closure can be called many times, specially when parallel downloads are enabled.
    /// Note that you can break the download using this, make sure that you are not changing
    /// configuration set by the [`DownloadTask`] code before use.
    #[inline]
    pub fn request_config<F>(mut self, config: F) -> Self
    where
        F: Fn(RequestBuilder) -> Result<RequestBuilder, Error> + Send + 'static,
    {
        self.request_config = Arc::new(config);
        self
    }

    fn normalize(&mut self) {
        if self.parallel_count == 0 {
            self.parallel_count = 1;
        }
        if self.max_speed < 57344 {
            self.max_speed = 57344;
        }
    }

    /// Start downloading the `response` body.
    pub fn spawn(mut self, response: Response) -> DownloadTask {
        self.normalize();
        DownloadTask::spawn(self, response)
    }

    /// Start downloading from the `uri` response body requested using HTTP GET.
    pub async fn get(self, uri: impl TryUri) -> Result<DownloadTask, Error> {
        let response = get(uri).await?;
        Ok(self.spawn(response))
    }

    /// Start downloading from the response body requested using the `request`.
    pub async fn req(self, request: Request) -> Result<DownloadTask, Error> {
        let response = send(request).await?;
        Ok(self.spawn(response))
    }
}

/// A [`DownloadTask`] that can be *reanimated* in another instance of the app.
pub struct FrozenDownloadTask {}
impl FrozenDownloadTask {
    /// Attempt to continue the download task.
    pub async fn resume(self) -> Result<DownloadTask, DownloadTaskError> {
        todo!()
    }
}

/// An error in [`DownloadTask`] or [`FrozenDownloadTask`]
pub enum DownloadTaskError {
    /// Download error.
    Io(std::io::Error),
    /// Lost connection with the task worker.
    ///
    /// The task worker closes on the first [`Io`] error.
    ///
    /// [`Io`]: Self::Io
    Closed,
}
impl fmt::Debug for DownloadTaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadTaskError::Io(e) => f.debug_tuple("Io").field(e).finish(),
            //DownloadTaskError::Panic(p) => write!(f, "Panic({:?})", panic_str(p)),
            DownloadTaskError::Closed => write!(f, "Closed"),
        }
    }
}
impl fmt::Display for DownloadTaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadTaskError::Io(e) => write!(f, "{}", e),
            //DownloadTaskError::Panic(p) => write!(f, "{}", panic_str(p)),
            DownloadTaskError::Closed => write!(f, "`DownloadTask` worker is closed due to error or panic"),
        }
    }
}
impl std::error::Error for DownloadTaskError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let DownloadTaskError::Io(e) = self {
            Some(e)
        } else {
            None
        }
    }
}
impl From<std::io::Error> for DownloadTaskError {
    fn from(e: std::io::Error) -> Self {
        DownloadTaskError::Io(e)
    }
}

/// Represents a running large file upload.
pub struct UploadTask {}
impl UploadTask {
    /// Start building an upload task using the [default client].
    ///
    /// [default client]: isahc_client
    #[inline]
    pub fn default() -> UploadTaskBuilder {
        UploadTaskBuilder::default()
    }

    /// Start building an upload task with a custom [`isahc`] client.
    ///
    /// [`isahc`]: https://docs.rs/isahc
    #[inline]
    pub fn with_client(client: isahc::HttpClient) -> UploadTaskBuilder {
        UploadTaskBuilder::new(client)
    }

    fn spawn(builder: UploadTaskBuilder, uri: Result<Uri, Error>) -> Self {
        todo!("{:?}, {:?}", builder, uri)
    }

    /// Send the next payload to upload.
    ///
    /// You can *pause* upload simply by not calling this method, if the connection was lost the task
    /// will attempt to retrieve it before continuing.
    pub async fn upload(&self, payload: Vec<u8>) -> Result<(), UploadTaskError> {
        todo!("{:?}", payload)
    }
}

/// Build a [`UploadTask`]
pub struct UploadTaskBuilder {
    client: isahc::HttpClient,
    channel_capacity: usize,
    max_speed: usize,
    request_config: Arc<dyn Fn(RequestBuilder) -> Result<RequestBuilder, Error> + Send>,
}
impl fmt::Debug for UploadTaskBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UploadTaskBuilder")
            .field("client", &self.client)
            .field("channel_capacity", &self.channel_capacity)
            .field("max_speed", &self.max_speed)
            .finish_non_exhaustive()
    }
}
impl Default for UploadTaskBuilder {
    fn default() -> Self {
        Self::new(isahc_client().clone())
    }
}
impl UploadTaskBuilder {
    fn new(client: isahc::HttpClient) -> Self {
        UploadTaskBuilder {
            client,
            channel_capacity: 8,
            max_speed: 0,
            request_config: Arc::new(Ok),
        }
    }

    /// Set the number of pending upload payloads that can wait in memory. If this
    /// capacity is reached the the [`upload`] method is pending until a payload is uploaded.
    ///
    /// Default is `8`.
    ///
    /// [`upload`]: UploadTask::upload
    pub fn channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity;
        self
    }

    /// Set the maximum upload speed, in bytes per second.
    ///
    /// Default is `usize::MAX` to indicate no limit. Minimal value is `57344` (56 kibibytes/s).
    #[inline]
    pub fn max_speed(mut self, bytes_per_sec: usize) -> Self {
        self.max_speed = bytes_per_sec;
        self
    }

    /// Set a closure that configures requests generated by the upload task.
    ///
    /// # Examples
    ///
    /// Set a custom header:
    ///
    /// ```
    /// # use zero_ui_core::task::http::*;
    /// # fn demo(builder: UploadTaskBuilder) -> UploadTaskBuilder {
    /// builder.request_config(|c| c.header("X-Foo-For", "Bar"))
    /// # }
    /// ```
    ///
    /// The closure can be called multiple times due to the task internal error recovery.
    ///
    /// Note that you can break the upload using this, make sure that you are not changing
    /// configuration set by the [`DownloadTask`] code before use.
    #[inline]
    pub fn request_config<F>(mut self, config: F) -> Self
    where
        F: Fn(RequestBuilder) -> Result<RequestBuilder, Error> + Send + 'static,
    {
        self.request_config = Arc::new(config);
        self
    }

    fn normalize(&mut self) {
        if self.max_speed < 57344 {
            self.max_speed = 57344;
        }
    }

    /// Start an idle upload task to the `uri`.
    pub fn spawn(mut self, uri: impl TryUri) -> UploadTask {
        self.normalize();
        UploadTask::spawn(self, uri.try_into())
    }
}

/// An error in [`UploadTask`].
pub struct UploadTaskError {}

/// Wrapper that implements [`ReadThenReceive`] for a [`Response`].
///
/// [`ReadThenReceive`]: super::ReadThenReceive
pub struct ReadThenReceive {
    response: Response,
}
impl ReadThenReceive {
    /// Read from the body of a [`Response`].
    pub fn new(response: Response) -> Self {
        ReadThenReceive { response }
    }

    /// HTTP GET the `uri` and read from the response body..
    pub async fn get(uri: impl TryUri) -> Result<Self, Error> {
        let uri = uri.try_into()?;
        get(uri).await.map(Self::new)
    }

    /// Read from the body of the response returned for the `request`.
    pub async fn req(request: Request) -> Result<Self, Error> {
        send(request).await.map(Self::new)
    }
}
#[async_trait]
impl super::ReadThenReceive for ReadThenReceive {
    type Error = self::Error;

    type Spawned = DownloadTask;

    async fn read_exact<const N: usize>(&mut self) -> Result<[u8; N], Self::Error> {
        let mut buf = [0; N];
        self.response.read_exact(&mut buf).await?;
        Ok(buf)
    }

    async fn read_exact_heap(&mut self, bytes: ByteLength) -> Result<Vec<u8>, Self::Error> {
        let mut buf = vec![0; bytes.0];
        self.response.read_exact(&mut buf).await?;
        Ok(buf)
    }

    fn spawn(self, payload_len: ByteLength, channel_capacity: usize) -> Self::Spawned {
        DownloadTask::default()
            .payload_len(payload_len)
            .channel_capacity(channel_capacity)
            .spawn(self.response)
    }
}
#[async_trait]
impl super::ReadThenReceive for Response {
    type Error = self::Error;

    type Spawned = DownloadTask;

    async fn read_exact<const N: usize>(&mut self) -> Result<[u8; N], Self::Error> {
        let mut buf = [0; N];
        self.read_exact(&mut buf).await?;
        Ok(buf)
    }

    async fn read_exact_heap(&mut self, bytes: ByteLength) -> Result<Vec<u8>, Self::Error> {
        let mut buf = vec![0; bytes.0];
        self.read_exact(&mut buf).await?;
        Ok(buf)
    }

    fn spawn(self, payload_len: ByteLength, channel_capacity: usize) -> Self::Spawned {
        DownloadTask::default()
            .payload_len(payload_len)
            .channel_capacity(channel_capacity)
            .spawn(self)
    }
}

/// Information about the state of an HTTP request or a [`DownloadTask`] or [`UploadTask`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metrics {
    /// Number of bytes uploaded / estimated total.
    pub upload_progress: (ByteLength, ByteLength),

    /// Average upload speed so far in bytes/second.
    pub upload_speed: ByteLength,

    /// Number of bytes downloaded / estimated total.
    pub download_progress: (ByteLength, ByteLength),

    /// Average download speed so far in bytes/second.
    pub download_speed: ByteLength,

    /// Total time from the start of the request until DNS name resolving was completed.
    ///
    /// When a redirect is followed, the time from each request is added together.
    pub name_lookup_time: Duration,

    /// Amount of time taken to establish a connection to the server (not including TLS connection time).
    ///
    /// When a redirect is followed, the time from each request is added together.
    pub connect_time: Duration,

    /// Amount of time spent on TLS handshakes.
    ///
    /// When a redirect is followed, the time from each request is added together.
    pub secure_connect_time: Duration,

    /// Time it took from the start of the request until the first byte is either sent or received.
    ///
    /// When a redirect is followed, the time from each request is added together.
    pub transfer_start_time: Duration,

    /// Amount of time spent performing the actual request transfer. The “transfer” includes
    /// both sending the request and receiving the response.
    ///
    /// When a redirect is followed, the time from each request is added together.
    pub transfer_time: Duration,

    /// Total time for the entire request. This will continuously increase until the entire
    /// response body is consumed and completed.
    ///
    /// When a redirect is followed, the time from each request is added together.
    pub total_time: Duration,

    /// If automatic redirect following is enabled, the total time taken for all redirection steps
    /// including name lookup, connect, pretransfer and transfer before final transaction was started.
    pub redirect_time: Duration,
}
impl Metrics {
    /// Init from [`isahc::Metrics`].
    pub fn from_isahc(m: &isahc::Metrics) -> Self {
        Self {
            upload_progress: {
                let (c, t) = m.upload_progress();
                ((c as usize).bytes(), (t as usize).bytes())
            },
            upload_speed: (m.upload_speed().round() as usize).bytes(),
            download_progress: {
                let (c, t) = m.download_progress();
                ((c as usize).bytes(), (t as usize).bytes())
            },
            download_speed: (m.download_speed().round() as usize).bytes(),
            name_lookup_time: m.name_lookup_time(),
            connect_time: m.connect_time(),
            secure_connect_time: m.secure_connect_time(),
            transfer_start_time: m.transfer_start_time(),
            transfer_time: m.transfer_time(),
            total_time: m.total_time(),
            redirect_time: m.redirect_time(),
        }
    }

    /// All zeros.
    pub fn zero() -> Self {
        Self {
            upload_progress: (0.bytes(), 0.bytes()),
            upload_speed: 0.bytes(),
            download_progress: (0.bytes(), 0.bytes()),
            download_speed: 0.bytes(),
            name_lookup_time: Duration::ZERO,
            connect_time: Duration::ZERO,
            secure_connect_time: Duration::ZERO,
            transfer_start_time: Duration::ZERO,
            transfer_time: Duration::ZERO,
            total_time: Duration::ZERO,
            redirect_time: Duration::ZERO,
        }
    }
}
impl From<isahc::Metrics> for Metrics {
    fn from(m: isahc::Metrics) -> Self {
        Metrics::from_isahc(&m)
    }
}
impl fmt::Display for Metrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut ws = false; // written something

        if self.upload_progress.0 != self.upload_progress.1 {
            write!(
                f,
                "upload: {} of {}, {}/s",
                self.upload_progress.0, self.upload_progress.1, self.upload_speed
            )?;
            ws = true;
        }
        if self.download_progress.0 != self.download_progress.1 {
            write!(
                f,
                "{}download: {} of {}, {}/s",
                if ws { "\n" } else { "" },
                self.download_progress.0,
                self.download_progress.1,
                self.download_speed
            )?;
            ws = true;
        }

        if !ws {
            if self.upload_progress.1.bytes() > 0 {
                write!(f, "uploaded: {}", self.upload_progress.1)?;
                ws = true;
            }
            if self.download_progress.1.bytes() > 0 {
                write!(f, "{}downloaded: {}", if ws { "\n" } else { "" }, self.download_progress.1)?;
                ws = true;
            }

            if ws {
                write!(f, "\ntotal time: {:?}", self.total_time)?;
            }
        }

        Ok(())
    }
}
