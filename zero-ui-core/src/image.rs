//! Image loading and cache.

use std::{
    env,
    future::Future,
    mem,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use parking_lot::Mutex;
use zero_ui_view_api::IpcBytes;

use crate::{
    app::{
        raw_events::{RAW_IMAGE_LOADED_EVENT, RAW_IMAGE_LOAD_ERROR_EVENT, RAW_IMAGE_METADATA_LOADED_EVENT},
        view_process::{ImageRequest, ViewImage, ViewProcess, ViewProcessOffline, VIEW_PROCESS, VIEW_PROCESS_INITED_EVENT},
        AppExtension,
    },
    app_local,
    crate_util::IdMap,
    event::EventUpdate,
    task::{self, fs, io::*, ui::UiTask},
    text::Txt,
    units::*,
    var::{types::WeakArcVar, *},
};

mod types;
pub use types::*;

mod render;
pub use render::{render_retain, IMAGE_RENDER};

/// Application extension that provides an image cache.
///
/// # Services
///
/// Services this extension provides.
///
/// * [`IMAGES`]
///
/// # Default
///
/// This extension is included in the [default app], events provided by it
/// are required by multiple other extensions.
///
/// [default app]: crate::app::App::default
#[derive(Default)]
pub struct ImageManager {}
impl AppExtension for ImageManager {
    fn init(&mut self) {
        IMAGES_SV.write().init(if VIEW_PROCESS.is_available() {
            Some(VIEW_PROCESS.clone())
        } else {
            None
        });
    }

    fn event_preview(&mut self, update: &mut EventUpdate) {
        if let Some(args) = RAW_IMAGE_METADATA_LOADED_EVENT.on(update) {
            let images = IMAGES_SV.read();

            if let Some(var) = images
                .decoding
                .iter()
                .map(|t| &t.image)
                .find(|v| v.with(|img| img.view.get().unwrap() == &args.image))
            {
                var.touch();
            }
        } else if let Some(args) = RAW_IMAGE_LOADED_EVENT.on(update) {
            let image = &args.image;

            // image finished decoding, remove from `decoding`
            // and notify image var value update.
            let mut images = IMAGES_SV.write();

            if let Some(i) = images
                .decoding
                .iter()
                .position(|t| t.image.with(|img| img.view.get().unwrap() == image))
            {
                let ImageDecodingTask { image, .. } = images.decoding.swap_remove(i);
                image.touch();
                image.with(|img| img.done_signal.set());
            }
        } else if let Some(args) = RAW_IMAGE_LOAD_ERROR_EVENT.on(update) {
            let image = &args.image;

            // image failed to decode, remove from `decoding`
            // and notify image var value update.
            let mut images = IMAGES_SV.write();

            if let Some(i) = images
                .decoding
                .iter()
                .position(|t| t.image.with(|img| img.view.get().unwrap() == image))
            {
                let ImageDecodingTask { image, .. } = images.decoding.swap_remove(i);
                image.touch();
                image.with(|img| {
                    img.done_signal.set();

                    if let Some(k) = &img.cache_key {
                        if let Some(e) = images.cache.get(k) {
                            e.error.store(true, Ordering::Relaxed);
                        }
                    }

                    tracing::error!("decode error: {:?}", img.error().unwrap());
                });
            }
        } else if let Some(args) = VIEW_PROCESS_INITED_EVENT.on(update) {
            if !args.is_respawn {
                return;
            }

            let mut images = IMAGES_SV.write();
            let images = &mut *images;
            images.cleanup_not_cached(true);
            images.download_accept.clear();
            let vp = images.view.as_mut().unwrap();
            let decoding_interrupted = mem::take(&mut images.decoding);
            for (img_var, max_decoded_len, downscale) in images
                .cache
                .values()
                .map(|e| (e.image.clone(), e.max_decoded_len, e.downscale))
                .chain(
                    images
                        .not_cached
                        .iter()
                        .filter_map(|e| e.image.upgrade().map(|v| (v, e.max_decoded_len, e.downscale))),
                )
            {
                let img = img_var.get();

                if let Some(view) = img.view.get() {
                    if view.generation() == args.generation {
                        continue; // already recovered, can this happen?
                    }
                    if let Some(e) = view.error() {
                        // respawned, but image was an error.
                        img_var.set(Image::dummy(Some(e.to_owned())));
                    } else if let Some(task) = decoding_interrupted.iter().find(|e| e.image.with(|img| img.view() == Some(view))) {
                        // respawned, but image was decoding, need to restart decode.

                        match vp.add_image(ImageRequest {
                            format: task.format.clone(),
                            data: task.data.clone(),
                            max_decoded_len: max_decoded_len.0 as u64,
                            downscale,
                        }) {
                            Ok(img) => {
                                img_var.set(Image::new(img));
                            }
                            Err(ViewProcessOffline) => { /*will receive another event.*/ }
                        }
                        images.decoding.push(ImageDecodingTask {
                            format: task.format.clone(),
                            data: task.data.clone(),
                            image: img_var,
                        });
                    } else {
                        // respawned and image was loaded.

                        let img_format = ImageDataFormat::Bgra8 {
                            size: view.size(),
                            ppi: view.ppi(),
                        };

                        let data = view.bgra8().unwrap();
                        let img = match vp.add_image(ImageRequest {
                            format: img_format.clone(),
                            data: data.clone(),
                            max_decoded_len: max_decoded_len.0 as u64,
                            downscale,
                        }) {
                            Ok(img) => img,
                            Err(ViewProcessOffline) => return, // we will receive another event.
                        };

                        img_var.set(Image::new(img));

                        images.decoding.push(ImageDecodingTask {
                            format: img_format,
                            data,
                            image: img_var,
                        });
                    }
                } // else { *is loading, will continue normally in self.update_preview()* }
            }
        } else {
            self.event_preview_render(update);
        }
    }

    fn update_preview(&mut self) {
        // update loading tasks:

        let mut images = IMAGES_SV.write();
        let images = &mut *images;
        let view = &images.view;
        let decoding = &mut images.decoding;
        let mut loading = Vec::with_capacity(images.loading.len());

        for t in mem::take(&mut images.loading) {
            t.task.lock().update();
            match t.task.into_inner().into_result() {
                Ok(d) => {
                    match d.r {
                        Ok(data) => {
                            if let Some(vp) = view {
                                // success and we have a view-process.
                                match vp.add_image(ImageRequest {
                                    format: d.format.clone(),
                                    data: data.clone(),
                                    max_decoded_len: t.max_decoded_len.0 as u64,
                                    downscale: t.downscale,
                                }) {
                                    Ok(img) => {
                                        // request sent, add to `decoding` will receive
                                        // `RawImageLoadedEvent` or `RawImageLoadErrorEvent` event
                                        // when done.
                                        t.image.modify(move |v| {
                                            v.to_mut().view.set(img).unwrap();
                                        });
                                    }
                                    Err(ViewProcessOffline) => {
                                        // will recover in ViewProcessInitedEvent
                                    }
                                }
                                decoding.push(ImageDecodingTask {
                                    format: d.format,
                                    data,
                                    image: t.image,
                                });
                            } else {
                                // success, but we are only doing `load_in_headless` validation.
                                let img = ViewImage::dummy(None);
                                t.image.modify(move |v| {
                                    let v = v.to_mut();
                                    v.view.set(img).unwrap();
                                    v.done_signal.set();
                                });
                            }
                        }
                        Err(e) => {
                            tracing::error!("load error: {e:?}");
                            // load error.
                            let img = ViewImage::dummy(Some(e));
                            t.image.modify(move |v| {
                                let v = v.to_mut();
                                v.view.set(img).unwrap();
                                v.done_signal.set();
                            });

                            // flag error for user retry
                            if let Some(k) = &t.image.with(|img| img.cache_key) {
                                if let Some(e) = images.cache.get(k) {
                                    e.error.store(true, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                }
                Err(task) => {
                    loading.push(ImageLoadingTask {
                        task: Mutex::new(task),
                        image: t.image,
                        max_decoded_len: t.max_decoded_len,
                        downscale: t.downscale,
                    });
                }
            }
        }
        images.loading = loading;
    }

    fn update(&mut self) {
        self.update_render();
    }
}

app_local! {
    static IMAGES_SV: ImagesService = ImagesService::new();
}

struct ImageLoadingTask {
    task: Mutex<UiTask<ImageData>>,
    image: ArcVar<Image>,
    max_decoded_len: ByteLength,
    downscale: Option<ImageDownscale>,
}

struct ImageDecodingTask {
    format: ImageDataFormat,
    data: IpcBytes,
    image: ArcVar<Image>,
}

struct CacheEntry {
    image: ArcVar<Image>,
    error: AtomicBool,
    max_decoded_len: ByteLength,
    downscale: Option<ImageDownscale>,
}

struct NotCachedEntry {
    image: WeakArcVar<Image>,
    max_decoded_len: ByteLength,
    downscale: Option<ImageDownscale>,
}

struct ImagesService {
    load_in_headless: ArcVar<bool>,
    limits: ArcVar<ImageLimits>,

    view: Option<ViewProcess>,
    download_accept: Txt,
    proxies: Vec<Box<dyn ImageCacheProxy>>,

    loading: Vec<ImageLoadingTask>,
    decoding: Vec<ImageDecodingTask>,
    cache: IdMap<ImageHash, CacheEntry>,
    not_cached: Vec<NotCachedEntry>,

    render: render::ImagesRender,
}
impl ImagesService {
    fn new() -> Self {
        Self {
            load_in_headless: var(false),
            limits: var(ImageLimits::default()),
            view: None,
            proxies: vec![],
            loading: vec![],
            decoding: vec![],
            download_accept: Txt::empty(),
            cache: IdMap::new(),
            not_cached: vec![],
            render: render::ImagesRender::default(),
        }
    }

    fn init(&mut self, view: Option<ViewProcess>) {
        self.view = view;
    }

    fn register(&mut self, key: ImageHash, image: ViewImage, downscale: Option<ImageDownscale>) -> Option<ImageVar> {
        let limits = self.limits.get();
        let limits = ImageLimits {
            max_encoded_len: limits.max_encoded_len,
            max_decoded_len: limits.max_decoded_len.max(image.bgra8().map(|b| b.len()).unwrap_or(0).bytes()),
            allow_path: PathFilter::BlockAll,
            #[cfg(http)]
            allow_uri: UriFilter::BlockAll,
        };
        let entry = CacheEntry {
            error: AtomicBool::new(image.is_error()),
            image: var(Image::new(image)),
            max_decoded_len: limits.max_decoded_len,
            downscale,
        };
        self.cache.insert(key, entry).map(|v| v.image.read_only())
    }

    fn detach(&mut self, image: ImageVar) -> ImageVar {
        if let Some(key) = &image.with(|i| i.cache_key) {
            let decoded_size = image.with(|img| img.bgra8().map(|b| b.len()).unwrap_or(0).bytes());
            let mut max_decoded_len = self.limits.with(|l| l.max_decoded_len.max(decoded_size));
            let mut downscale = None;

            if let Some(e) = self.cache.get(key) {
                max_decoded_len = e.max_decoded_len;
                downscale = e.downscale;

                // is cached, `clean` if is only external reference.
                if image.strong_count() == 2 {
                    self.cache.remove(key);
                }
            }

            // remove `cache_key` from image, this clones the `Image` only-if is still in cache.
            let mut img = image.into_value();
            img.cache_key = None;
            let img = var(img);
            self.not_cached.push(NotCachedEntry {
                image: img.downgrade(),
                max_decoded_len,
                downscale,
            });
            img.read_only()
        } else {
            // already not cached
            image
        }
    }

    fn proxy_then_remove(&mut self, key: &ImageHash, purge: bool) -> bool {
        for proxy in &mut self.proxies {
            let r = proxy.remove(key, purge);
            match r {
                ProxyRemoveResult::None => continue,
                ProxyRemoveResult::Remove(r, p) => return self.proxied_remove(&r, p),
                ProxyRemoveResult::Removed => return true,
            }
        }
        self.proxied_remove(key, purge)
    }
    fn proxied_remove(&mut self, key: &ImageHash, purge: bool) -> bool {
        if purge || self.cache.get(key).map(|v| v.image.strong_count() > 1).unwrap_or(false) {
            self.cache.remove(key).is_some()
        } else {
            false
        }
    }

    fn proxy_then_get(
        &mut self,
        source: ImageSource,
        mode: ImageCacheMode,
        limits: ImageLimits,
        downscale: Option<ImageDownscale>,
    ) -> ImageVar {
        let source = match source {
            ImageSource::Read(path) => {
                let path = crate::crate_util::absolute_path(&path, || env::current_dir().expect("could not access current dir"), true);
                if !limits.allow_path.allows(&path) {
                    let error = format!("limits filter blocked `{}`", path.display());
                    tracing::error!("{error}");
                    return var(Image::dummy(Some(error))).read_only();
                }
                ImageSource::Read(path)
            }
            #[cfg(http)]
            ImageSource::Download(uri, accepts) => {
                if !limits.allow_uri.allows(&uri) {
                    let error = format!("limits filter blocked `{uri}`");
                    tracing::error!("{error}");
                    return var(Image::dummy(Some(error))).read_only();
                }
                ImageSource::Download(uri, accepts)
            }
            ImageSource::Image(r) => return r,
            source => source,
        };

        let key = source.hash128(downscale).unwrap();
        for proxy in &mut self.proxies {
            let r = proxy.get(&key, &source, mode, downscale);
            match r {
                ProxyGetResult::None => continue,
                ProxyGetResult::Cache(source, mode, downscale) => {
                    return self.proxied_get(source.hash128(downscale).unwrap(), source, mode, limits, downscale)
                }
                ProxyGetResult::Image(img) => return img,
            }
        }
        self.proxied_get(key, source, mode, limits, downscale)
    }
    fn proxied_get(
        &mut self,
        key: ImageHash,
        source: ImageSource,
        mode: ImageCacheMode,
        limits: ImageLimits,
        downscale: Option<ImageDownscale>,
    ) -> ImageVar {
        match mode {
            ImageCacheMode::Cache => {
                if let Some(v) = self.cache.get(&key) {
                    return v.image.read_only();
                }
            }
            ImageCacheMode::Retry => {
                if let Some(e) = self.cache.get(&key) {
                    if !e.error.load(Ordering::Relaxed) {
                        return e.image.read_only();
                    }
                }
            }
            ImageCacheMode::Ignore | ImageCacheMode::Reload => {}
        }

        if self.view.is_none() && !self.load_in_headless.get() {
            tracing::warn!("loading dummy image, set `load_in_headless=true` to actually load without renderer");

            let dummy = var(Image::new(ViewImage::dummy(None)));
            self.cache.insert(
                key,
                CacheEntry {
                    image: dummy.clone(),
                    error: AtomicBool::new(false),
                    max_decoded_len: limits.max_decoded_len,
                    downscale,
                },
            );
            return dummy.read_only();
        }

        let max_encoded_size = limits.max_encoded_len;

        match source {
            ImageSource::Read(path) => self.load_task(
                key,
                mode,
                limits.max_decoded_len,
                downscale,
                task::run(async move {
                    let mut r = ImageData {
                        format: path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|s| ImageDataFormat::FileExtension(s.to_owned()))
                            .unwrap_or(ImageDataFormat::Unknown),
                        r: Err(String::new()),
                    };

                    let mut file = match fs::File::open(path).await {
                        Ok(f) => f,
                        Err(e) => {
                            r.r = Err(e.to_string());
                            return r;
                        }
                    };

                    let len = match file.metadata().await {
                        Ok(m) => m.len() as usize,
                        Err(e) => {
                            r.r = Err(e.to_string());
                            return r;
                        }
                    };

                    if len > max_encoded_size.0 {
                        r.r = Err(format!("file size `{}` exceeds the limit of `{max_encoded_size}`", len.bytes()));
                        return r;
                    }

                    let mut data = Vec::with_capacity(len);
                    r.r = match file.read_to_end(&mut data).await {
                        Ok(_) => Ok(IpcBytes::from_vec(data)),
                        Err(e) => Err(e.to_string()),
                    };

                    r
                }),
            ),
            #[cfg(http)]
            ImageSource::Download(uri, accept) => {
                let accept = accept.unwrap_or_else(|| self.download_accept());

                self.load_task(
                    key,
                    mode,
                    limits.max_decoded_len,
                    downscale,
                    task::run(async move {
                        let mut r = ImageData {
                            format: ImageDataFormat::Unknown,
                            r: Err(String::new()),
                        };

                        let request = task::http::Request::get(uri)
                            .unwrap()
                            .header(task::http::header::ACCEPT, accept)
                            .unwrap()
                            .max_length(max_encoded_size)
                            .build();

                        match task::http::send(request).await {
                            Ok(mut rsp) => {
                                if let Some(m) = rsp.headers().get(&task::http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
                                    let m = m.to_lowercase();
                                    if m.starts_with("image/") {
                                        r.format = ImageDataFormat::MimeType(m);
                                    }
                                }

                                match rsp.bytes().await {
                                    Ok(d) => r.r = Ok(IpcBytes::from_vec(d)),
                                    Err(e) => {
                                        r.r = Err(format!("download error: {e}"));
                                    }
                                }

                                let _ = rsp.consume().await;
                            }
                            Err(e) => {
                                r.r = Err(format!("request error: {e}"));
                            }
                        }

                        r
                    }),
                )
            }
            ImageSource::Static(_, bytes, fmt) => {
                let r = ImageData {
                    format: fmt,
                    r: Ok(IpcBytes::from_slice(bytes)),
                };
                self.load_task(key, mode, limits.max_decoded_len, downscale, async { r })
            }
            ImageSource::Data(_, bytes, fmt) => {
                let r = ImageData {
                    format: fmt,
                    r: Ok(IpcBytes::from_slice(&bytes)),
                };
                self.load_task(key, mode, limits.max_decoded_len, downscale, async { r })
            }
            ImageSource::Render(rfn, args) => {
                let img = self.new_cache_image(key, mode, limits.max_decoded_len, downscale);
                self.render_img(clmv!(rfn, || rfn(&args.unwrap_or_default())), &img);
                img.read_only()
            }
            ImageSource::Image(_) => unreachable!(),
        }
    }

    #[cfg(http)]
    fn download_accept(&mut self) -> Txt {
        if self.download_accept.is_empty() {
            if let Some(view) = &self.view {
                let mut r = String::new();
                let mut fmts = view.image_decoders().unwrap_or_default().into_iter();
                if let Some(fmt) = fmts.next() {
                    r.push_str("image/");
                    r.push_str(&fmt);
                    for fmt in fmts {
                        r.push_str(",image/");
                        r.push_str(&fmt);
                    }
                    self.download_accept = r.into();
                }
            }
            if self.download_accept.is_empty() {
                self.download_accept = "image/*".into();
            }
        }
        self.download_accept.clone()
    }

    fn cleanup_not_cached(&mut self, force: bool) {
        if force || self.not_cached.len() > 1000 {
            self.not_cached.retain(|c| c.image.strong_count() > 0);
        }
    }

    fn new_cache_image(
        &mut self,
        key: ImageHash,
        mode: ImageCacheMode,
        max_decoded_len: ByteLength,
        downscale: Option<ImageDownscale>,
    ) -> ArcVar<Image> {
        self.cleanup_not_cached(false);

        if let ImageCacheMode::Reload = mode {
            self.cache
                .entry(key)
                .or_insert_with(|| CacheEntry {
                    image: var(Image::new_none(Some(key))),
                    error: AtomicBool::new(false),
                    max_decoded_len,
                    downscale,
                })
                .image
                .clone()
        } else if let ImageCacheMode::Ignore = mode {
            let img = var(Image::new_none(None));
            self.not_cached.push(NotCachedEntry {
                image: img.downgrade(),
                max_decoded_len,
                downscale,
            });
            img
        } else {
            let img = var(Image::new_none(Some(key)));
            self.cache.insert(
                key,
                CacheEntry {
                    image: img.clone(),
                    error: AtomicBool::new(false),
                    max_decoded_len,
                    downscale,
                },
            );
            img
        }
    }

    /// The `fetch_bytes` future is polled in the UI thread, use `task::run` for futures that poll a lot.
    fn load_task(
        &mut self,
        key: ImageHash,
        mode: ImageCacheMode,
        max_decoded_len: ByteLength,
        downscale: Option<ImageDownscale>,
        fetch_bytes: impl Future<Output = ImageData> + Send + 'static,
    ) -> ImageVar {
        let img = self.new_cache_image(key, mode, max_decoded_len, downscale);
        let r = img.read_only();

        self.loading.push(ImageLoadingTask {
            task: Mutex::new(UiTask::new(None, fetch_bytes)),
            image: img,
            max_decoded_len,
            downscale,
        });

        r
    }
}

/// Image loading, cache and render service.
///
/// If the app is running without a [`ViewProcess`] all images are dummy, see [`load_in_headless`] for
/// details.
///
/// [`load_in_headless`]: IMAGES::load_in_headless
pub struct IMAGES;
impl IMAGES {
    /// If should still download/read image bytes in headless/renderless mode.
    ///
    /// When an app is in headless mode without renderer no [`ViewProcess`] is available, so
    /// images cannot be decoded, in this case all images are the [`dummy`] image and no attempt
    /// to download/read the image files is made. You can enable loading in headless tests to detect
    /// IO errors, in this case if there is an error acquiring the image file the image will be a
    /// [`dummy`] with error.
    ///
    /// [`dummy`]: IMAGES::dummy
    pub fn load_in_headless(&self) -> ArcVar<bool> {
        IMAGES_SV.read().load_in_headless.clone()
    }

    /// Default loading and decoding limits for each image.
    pub fn limits(&self) -> ArcVar<ImageLimits> {
        IMAGES_SV.read().limits.clone()
    }

    /// Returns a dummy image that reports it is loaded or an error.
    pub fn dummy(&self, error: Option<String>) -> ImageVar {
        var(Image::dummy(error)).read_only()
    }

    /// Cache or load an image file from a file system `path`.
    pub fn read(&self, path: impl Into<PathBuf>) -> ImageVar {
        self.cache(path.into())
    }

    /// Get a cached `uri` or download it.
    ///
    /// Optionally define the HTTP ACCEPT header, if not set all image formats supported by the view-process
    /// backend are accepted.
    #[cfg(http)]
    pub fn download(&self, uri: impl task::http::TryUri, accept: Option<Txt>) -> ImageVar {
        match uri.try_uri() {
            Ok(uri) => self.cache(ImageSource::Download(uri, accept)),
            Err(e) => self.dummy(Some(e.to_string())),
        }
    }

    /// Get a cached image from `&'static [u8]` data.
    ///
    /// The data can be any of the formats described in [`ImageDataFormat`].
    ///
    /// The image key is a [`ImageHash`] of the image data.
    ///
    /// # Examples
    ///
    /// Get an image from a PNG file embedded in the app executable using [`include_bytes!`].
    ///
    /// ```
    /// # use zero_ui_core::{image::*};
    /// # macro_rules! include_bytes { ($tt:tt) => { &[] } }
    /// # fn demo() {
    /// let image_var = IMAGES.from_static(include_bytes!("ico.png"), "png");
    /// # }
    pub fn from_static(&self, data: &'static [u8], format: impl Into<ImageDataFormat>) -> ImageVar {
        self.cache((data, format.into()))
    }

    /// Get a cached image from shared data.
    ///
    /// The image key is a [`ImageHash`] of the image data. The data reference is held only until the image is decoded.
    ///
    /// The data can be any of the formats described in [`ImageDataFormat`].
    pub fn from_data(&self, data: Arc<Vec<u8>>, format: impl Into<ImageDataFormat>) -> ImageVar {
        self.cache((data, format.into()))
    }

    /// Get a cached image or add it to the cache.
    pub fn cache(&self, source: impl Into<ImageSource>) -> ImageVar {
        self.image(source, ImageCacheMode::Cache, None, None)
    }

    /// Get a cached image or add it to the cache or retry if the cached image is an error.
    pub fn retry(&self, source: impl Into<ImageSource>) -> ImageVar {
        self.image(source, ImageCacheMode::Retry, None, None)
    }

    /// Load an image, if it was already cached update the cached image with the reloaded data.
    pub fn reload(&self, source: impl Into<ImageSource>) -> ImageVar {
        self.image(source, ImageCacheMode::Reload, None, None)
    }

    /// Get or load an image.
    ///
    /// If `limits` is `None` the [`IMAGES.limits`] is used.
    pub fn image(
        &self,
        source: impl Into<ImageSource>,
        cache_mode: impl Into<ImageCacheMode>,
        limits: Option<ImageLimits>,
        downscale: Option<ImageDownscale>,
    ) -> ImageVar {
        let limits = limits.unwrap_or_else(|| IMAGES_SV.read().limits.get());
        IMAGES_SV
            .write()
            .proxy_then_get(source.into(), cache_mode.into(), limits, downscale)
    }

    /// Associate the `image` with the `key` in the cache.
    ///
    /// Returns `Some(previous)` if the `key` was already associated with an image.
    pub fn register(&self, key: ImageHash, image: ViewImage) -> Option<ImageVar> {
        IMAGES_SV.write().register(key, image, None)
    }

    /// Remove the image from the cache, if it is only held by the cache.
    ///
    /// You can use [`ImageSource::hash128_read`] and [`ImageSource::hash128_download`] to get the `key`
    /// for files or downloads.
    ///
    /// Returns `true` if the image was removed.
    pub fn clean(&self, key: ImageHash) -> bool {
        IMAGES_SV.write().proxy_then_remove(&key, false)
    }

    /// Remove the image from the cache, even if it is still referenced outside of the cache.
    ///
    /// You can use [`ImageSource::hash128_read`] and [`ImageSource::hash128_download`] to get the `key`
    /// for files or downloads.
    ///
    /// Returns `true` if the image was cached.
    pub fn purge(&self, key: &ImageHash) -> bool {
        IMAGES_SV.write().proxy_then_remove(key, true)
    }

    /// Gets the cache key of an image.
    pub fn cache_key(&self, image: &Image) -> Option<ImageHash> {
        if let Some(key) = &image.cache_key {
            if IMAGES_SV.read().cache.contains_key(key) {
                return Some(*key);
            }
        }
        None
    }

    /// If the image is cached.
    pub fn is_cached(&self, image: &Image) -> bool {
        image
            .cache_key
            .as_ref()
            .map(|k| IMAGES_SV.read().cache.contains_key(k))
            .unwrap_or(false)
    }

    /// Returns an image that is not cached.
    ///
    /// If the `image` is the only reference returns it and removes it from the cache. If there are other
    /// references a new [`ImageVar`] is generated from a clone of the image.
    pub fn detach(&self, image: ImageVar) -> ImageVar {
        IMAGES_SV.write().detach(image)
    }

    /// Clear cached images that are not referenced outside of the cache.
    pub fn clean_all(&self) {
        let mut img = IMAGES_SV.write();
        img.proxies.iter_mut().for_each(|p| p.clear(false));
        img.cache.retain(|_, v| v.image.strong_count() > 1);
    }

    /// Clear all cached images, including images that are still referenced outside of the cache.
    ///
    /// Image memory only drops when all strong references are removed, so if an image is referenced
    /// outside of the cache it will merely be disconnected from the cache by this method.
    pub fn purge_all(&self) {
        let mut img = IMAGES_SV.write();
        img.cache.clear();
        img.proxies.iter_mut().for_each(|p| p.clear(true));
    }

    /// Add a cache proxy.
    ///
    /// Proxies can intercept cache requests and map to a different request or return an image directly.
    pub fn install_proxy(&self, proxy: Box<dyn ImageCacheProxy>) {
        IMAGES_SV.write().proxies.push(proxy);
    }
}
struct ImageData {
    format: ImageDataFormat,
    r: std::result::Result<IpcBytes, String>,
}
