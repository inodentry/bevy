#[cfg(feature = "basis-universal")]
use super::basis::*;
#[cfg(feature = "dds")]
use super::dds::*;
#[cfg(feature = "ktx2")]
use super::ktx2::*;

use crate::{
    color::Color,
    render_asset::{PrepareAssetError, RenderAsset},
    render_resource::{Sampler, Texture, TextureView},
    renderer::{RenderDevice, RenderQueue},
    texture::BevyDefault,
};
use bevy_asset::Asset;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::system::{lifetimeless::SRes, Resource, SystemParamItem};
use bevy_math::{UVec2, UVec3, Vec2};
use bevy_reflect::Reflect;
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use thiserror::Error;
use wgpu::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor};

pub const TEXTURE_ASSET_INDEX: u64 = 0;
pub const SAMPLER_ASSET_INDEX: u64 = 1;

#[derive(Debug, Serialize, Deserialize, Copy, Clone)]
pub enum ImageFormat {
    Avif,
    Basis,
    Bmp,
    Dds,
    Farbfeld,
    Gif,
    OpenExr,
    Hdr,
    Ico,
    Jpeg,
    Ktx2,
    Png,
    Pnm,
    Tga,
    Tiff,
    WebP,
}

impl ImageFormat {
    pub fn from_mime_type(mime_type: &str) -> Option<Self> {
        Some(match mime_type.to_ascii_lowercase().as_str() {
            "image/bmp" | "image/x-bmp" => ImageFormat::Bmp,
            "image/vnd-ms.dds" => ImageFormat::Dds,
            "image/jpeg" => ImageFormat::Jpeg,
            "image/ktx2" => ImageFormat::Ktx2,
            "image/png" => ImageFormat::Png,
            "image/x-exr" => ImageFormat::OpenExr,
            "image/x-targa" | "image/x-tga" => ImageFormat::Tga,
            _ => return None,
        })
    }

    pub fn from_extension(extension: &str) -> Option<Self> {
        Some(match extension.to_ascii_lowercase().as_str() {
            "avif" => ImageFormat::Avif,
            "basis" => ImageFormat::Basis,
            "bmp" => ImageFormat::Bmp,
            "dds" => ImageFormat::Dds,
            "ff" | "farbfeld" => ImageFormat::Farbfeld,
            "gif" => ImageFormat::Gif,
            "exr" => ImageFormat::OpenExr,
            "hdr" => ImageFormat::Hdr,
            "ico" => ImageFormat::Ico,
            "jpg" | "jpeg" => ImageFormat::Jpeg,
            "ktx2" => ImageFormat::Ktx2,
            "pbm" | "pam" | "ppm" | "pgm" => ImageFormat::Pnm,
            "png" => ImageFormat::Png,
            "tga" => ImageFormat::Tga,
            "tif" | "tiff" => ImageFormat::Tiff,
            "webp" => ImageFormat::WebP,
            _ => return None,
        })
    }

    pub fn as_image_crate_format(&self) -> Option<image::ImageFormat> {
        Some(match self {
            ImageFormat::Avif => image::ImageFormat::Avif,
            ImageFormat::Bmp => image::ImageFormat::Bmp,
            ImageFormat::Dds => image::ImageFormat::Dds,
            ImageFormat::Farbfeld => image::ImageFormat::Farbfeld,
            ImageFormat::Gif => image::ImageFormat::Gif,
            ImageFormat::OpenExr => image::ImageFormat::OpenExr,
            ImageFormat::Hdr => image::ImageFormat::Hdr,
            ImageFormat::Ico => image::ImageFormat::Ico,
            ImageFormat::Jpeg => image::ImageFormat::Jpeg,
            ImageFormat::Png => image::ImageFormat::Png,
            ImageFormat::Pnm => image::ImageFormat::Pnm,
            ImageFormat::Tga => image::ImageFormat::Tga,
            ImageFormat::Tiff => image::ImageFormat::Tiff,
            ImageFormat::WebP => image::ImageFormat::WebP,
            ImageFormat::Basis | ImageFormat::Ktx2 => return None,
        })
    }
}

#[derive(Asset, Reflect, Debug, Clone)]
#[reflect_value]
pub struct Image {
    pub data: Vec<u8>,
    // TODO: this nesting makes accessing Image metadata verbose. Either flatten out descriptor or add accessors
    pub texture_descriptor: wgpu::TextureDescriptor<'static>,
    /// The [`ImageSampler`] to use during rendering.
    pub sampler: ImageSampler,
    pub texture_view_descriptor: Option<wgpu::TextureViewDescriptor<'static>>,
}

/// Used in [`Image`], this determines what image sampler to use when rendering. The default setting,
/// [`ImageSampler::Default`], will read the sampler from the [`ImagePlugin`](super::ImagePlugin) at setup.
/// Setting this to [`ImageSampler::Descriptor`] will override the global default descriptor for this [`Image`].
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub enum ImageSampler {
    /// Default image sampler, derived from the [`ImagePlugin`](super::ImagePlugin) setup.
    #[default]
    Default,
    /// Custom sampler for this image which will override global default.
    Descriptor(ImageSamplerDescriptor),
}

impl ImageSampler {
    /// Returns an image sampler with [`ImageFilterMode::Linear`] min and mag filters
    #[inline]
    pub fn linear() -> ImageSampler {
        ImageSampler::Descriptor(ImageSamplerDescriptor::linear())
    }

    /// Returns an image sampler with [`ImageFilterMode::Nearest`] min and mag filters
    #[inline]
    pub fn nearest() -> ImageSampler {
        ImageSampler::Descriptor(ImageSamplerDescriptor::nearest())
    }
}

/// A rendering resource for the default image sampler which is set during renderer
/// initialization.
///
/// The [`ImagePlugin`](super::ImagePlugin) can be set during app initialization to change the default
/// image sampler.
#[derive(Resource, Debug, Clone, Deref, DerefMut)]
pub struct DefaultImageSampler(pub(crate) Sampler);

/// How edges should be handled in texture addressing.
///
/// This type mirrors [`wgpu::AddressMode`].
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum ImageAddressMode {
    /// Clamp the value to the edge of the texture.
    ///
    /// -0.25 -> 0.0
    /// 1.25  -> 1.0
    #[default]
    ClampToEdge,
    /// Repeat the texture in a tiling fashion.
    ///
    /// -0.25 -> 0.75
    /// 1.25 -> 0.25
    Repeat,
    /// Repeat the texture, mirroring it every repeat.
    ///
    /// -0.25 -> 0.25
    /// 1.25 -> 0.75
    MirrorRepeat,
    /// Clamp the value to the border of the texture
    /// Requires the wgpu feature [`wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER`].
    ///
    /// -0.25 -> border
    /// 1.25 -> border
    ClampToBorder,
}

/// Texel mixing mode when sampling between texels.
///
/// This type mirrors [`wgpu::FilterMode`].
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum ImageFilterMode {
    /// Nearest neighbor sampling.
    ///
    /// This creates a pixelated effect when used as a mag filter.
    #[default]
    Nearest,
    /// Linear Interpolation.
    ///
    /// This makes textures smooth but blurry when used as a mag filter.
    Linear,
}

/// Comparison function used for depth and stencil operations.
///
/// This type mirrors [`wgpu::CompareFunction`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ImageCompareFunction {
    /// Function never passes
    Never,
    /// Function passes if new value less than existing value
    Less,
    /// Function passes if new value is equal to existing value. When using
    /// this compare function, make sure to mark your Vertex Shader's `@builtin(position)`
    /// output as `@invariant` to prevent artifacting.
    Equal,
    /// Function passes if new value is less than or equal to existing value
    LessEqual,
    /// Function passes if new value is greater than existing value
    Greater,
    /// Function passes if new value is not equal to existing value. When using
    /// this compare function, make sure to mark your Vertex Shader's `@builtin(position)`
    /// output as `@invariant` to prevent artifacting.
    NotEqual,
    /// Function passes if new value is greater than or equal to existing value
    GreaterEqual,
    /// Function always passes
    Always,
}

/// Color variation to use when the sampler addressing mode is [`ImageAddressMode::ClampToBorder`].
///
/// This type mirrors [`wgpu::SamplerBorderColor`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ImageSamplerBorderColor {
    /// RGBA color `[0, 0, 0, 0]`.
    TransparentBlack,
    /// RGBA color `[0, 0, 0, 1]`.
    OpaqueBlack,
    /// RGBA color `[1, 1, 1, 1]`.
    OpaqueWhite,
    /// On the Metal wgpu backend, this is equivalent to [`Self::TransparentBlack`] for
    /// textures that have an alpha component, and equivalent to [`Self::OpaqueBlack`]
    /// for textures that do not have an alpha component. On other backends,
    /// this is equivalent to [`Self::TransparentBlack`]. Requires
    /// [`wgpu::Features::ADDRESS_MODE_CLAMP_TO_ZERO`]. Not supported on the web.
    Zero,
}

/// Indicates to an [`ImageLoader`](super::ImageLoader) how an [`Image`] should be sampled.
/// As this type is part of the [`ImageLoaderSettings`](super::ImageLoaderSettings),
/// it will be serialized to an image asset `.meta` file which might require a migration in case of
/// a breaking change.
///
/// This types mirrors [`wgpu::SamplerDescriptor`], but that might change in future versions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageSamplerDescriptor {
    pub label: Option<String>,
    /// How to deal with out of bounds accesses in the u (i.e. x) direction.
    pub address_mode_u: ImageAddressMode,
    /// How to deal with out of bounds accesses in the v (i.e. y) direction.
    pub address_mode_v: ImageAddressMode,
    /// How to deal with out of bounds accesses in the w (i.e. z) direction.
    pub address_mode_w: ImageAddressMode,
    /// How to filter the texture when it needs to be magnified (made larger).
    pub mag_filter: ImageFilterMode,
    /// How to filter the texture when it needs to be minified (made smaller).
    pub min_filter: ImageFilterMode,
    /// How to filter between mip map levels
    pub mipmap_filter: ImageFilterMode,
    /// Minimum level of detail (i.e. mip level) to use.
    pub lod_min_clamp: f32,
    /// Maximum level of detail (i.e. mip level) to use.
    pub lod_max_clamp: f32,
    /// If this is enabled, this is a comparison sampler using the given comparison function.
    pub compare: Option<ImageCompareFunction>,
    /// Must be at least 1. If this is not 1, all filter modes must be linear.
    pub anisotropy_clamp: u16,
    /// Border color to use when `address_mode`` is [`ImageAddressMode::ClampToBorder`].
    pub border_color: Option<ImageSamplerBorderColor>,
}

impl Default for ImageSamplerDescriptor {
    fn default() -> Self {
        Self {
            address_mode_u: Default::default(),
            address_mode_v: Default::default(),
            address_mode_w: Default::default(),
            mag_filter: Default::default(),
            min_filter: Default::default(),
            mipmap_filter: Default::default(),
            lod_min_clamp: 0.0,
            lod_max_clamp: 32.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
            label: None,
        }
    }
}

impl ImageSamplerDescriptor {
    /// Returns a sampler descriptor with [`Linear`](crate::render_resource::FilterMode::Linear) min and mag filters
    #[inline]
    pub fn linear() -> ImageSamplerDescriptor {
        ImageSamplerDescriptor {
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..Default::default()
        }
    }

    /// Returns a sampler descriptor with [`Nearest`](crate::render_resource::FilterMode::Nearest) min and mag filters
    #[inline]
    pub fn nearest() -> ImageSamplerDescriptor {
        ImageSamplerDescriptor {
            mag_filter: ImageFilterMode::Nearest,
            min_filter: ImageFilterMode::Nearest,
            mipmap_filter: ImageFilterMode::Nearest,
            ..Default::default()
        }
    }

    pub fn as_wgpu(&self) -> wgpu::SamplerDescriptor {
        wgpu::SamplerDescriptor {
            label: self.label.as_deref(),
            address_mode_u: self.address_mode_u.into(),
            address_mode_v: self.address_mode_v.into(),
            address_mode_w: self.address_mode_w.into(),
            mag_filter: self.mag_filter.into(),
            min_filter: self.min_filter.into(),
            mipmap_filter: self.mipmap_filter.into(),
            lod_min_clamp: self.lod_min_clamp,
            lod_max_clamp: self.lod_max_clamp,
            compare: self.compare.map(Into::into),
            anisotropy_clamp: self.anisotropy_clamp,
            border_color: self.border_color.map(Into::into),
        }
    }
}

impl From<ImageAddressMode> for wgpu::AddressMode {
    fn from(value: ImageAddressMode) -> Self {
        match value {
            ImageAddressMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
            ImageAddressMode::Repeat => wgpu::AddressMode::Repeat,
            ImageAddressMode::MirrorRepeat => wgpu::AddressMode::MirrorRepeat,
            ImageAddressMode::ClampToBorder => wgpu::AddressMode::ClampToBorder,
        }
    }
}

impl From<ImageFilterMode> for wgpu::FilterMode {
    fn from(value: ImageFilterMode) -> Self {
        match value {
            ImageFilterMode::Nearest => wgpu::FilterMode::Nearest,
            ImageFilterMode::Linear => wgpu::FilterMode::Linear,
        }
    }
}

impl From<ImageCompareFunction> for wgpu::CompareFunction {
    fn from(value: ImageCompareFunction) -> Self {
        match value {
            ImageCompareFunction::Never => wgpu::CompareFunction::Never,
            ImageCompareFunction::Less => wgpu::CompareFunction::Less,
            ImageCompareFunction::Equal => wgpu::CompareFunction::Equal,
            ImageCompareFunction::LessEqual => wgpu::CompareFunction::LessEqual,
            ImageCompareFunction::Greater => wgpu::CompareFunction::Greater,
            ImageCompareFunction::NotEqual => wgpu::CompareFunction::NotEqual,
            ImageCompareFunction::GreaterEqual => wgpu::CompareFunction::GreaterEqual,
            ImageCompareFunction::Always => wgpu::CompareFunction::Always,
        }
    }
}

impl From<ImageSamplerBorderColor> for wgpu::SamplerBorderColor {
    fn from(value: ImageSamplerBorderColor) -> Self {
        match value {
            ImageSamplerBorderColor::TransparentBlack => wgpu::SamplerBorderColor::TransparentBlack,
            ImageSamplerBorderColor::OpaqueBlack => wgpu::SamplerBorderColor::OpaqueBlack,
            ImageSamplerBorderColor::OpaqueWhite => wgpu::SamplerBorderColor::OpaqueWhite,
            ImageSamplerBorderColor::Zero => wgpu::SamplerBorderColor::Zero,
        }
    }
}

impl From<wgpu::AddressMode> for ImageAddressMode {
    fn from(value: wgpu::AddressMode) -> Self {
        match value {
            wgpu::AddressMode::ClampToEdge => ImageAddressMode::ClampToEdge,
            wgpu::AddressMode::Repeat => ImageAddressMode::Repeat,
            wgpu::AddressMode::MirrorRepeat => ImageAddressMode::MirrorRepeat,
            wgpu::AddressMode::ClampToBorder => ImageAddressMode::ClampToBorder,
        }
    }
}

impl From<wgpu::FilterMode> for ImageFilterMode {
    fn from(value: wgpu::FilterMode) -> Self {
        match value {
            wgpu::FilterMode::Nearest => ImageFilterMode::Nearest,
            wgpu::FilterMode::Linear => ImageFilterMode::Linear,
        }
    }
}

impl From<wgpu::CompareFunction> for ImageCompareFunction {
    fn from(value: wgpu::CompareFunction) -> Self {
        match value {
            wgpu::CompareFunction::Never => ImageCompareFunction::Never,
            wgpu::CompareFunction::Less => ImageCompareFunction::Less,
            wgpu::CompareFunction::Equal => ImageCompareFunction::Equal,
            wgpu::CompareFunction::LessEqual => ImageCompareFunction::LessEqual,
            wgpu::CompareFunction::Greater => ImageCompareFunction::Greater,
            wgpu::CompareFunction::NotEqual => ImageCompareFunction::NotEqual,
            wgpu::CompareFunction::GreaterEqual => ImageCompareFunction::GreaterEqual,
            wgpu::CompareFunction::Always => ImageCompareFunction::Always,
        }
    }
}

impl From<wgpu::SamplerBorderColor> for ImageSamplerBorderColor {
    fn from(value: wgpu::SamplerBorderColor) -> Self {
        match value {
            wgpu::SamplerBorderColor::TransparentBlack => ImageSamplerBorderColor::TransparentBlack,
            wgpu::SamplerBorderColor::OpaqueBlack => ImageSamplerBorderColor::OpaqueBlack,
            wgpu::SamplerBorderColor::OpaqueWhite => ImageSamplerBorderColor::OpaqueWhite,
            wgpu::SamplerBorderColor::Zero => ImageSamplerBorderColor::Zero,
        }
    }
}

impl<'a> From<wgpu::SamplerDescriptor<'a>> for ImageSamplerDescriptor {
    fn from(value: wgpu::SamplerDescriptor) -> Self {
        ImageSamplerDescriptor {
            label: value.label.map(|l| l.to_string()),
            address_mode_u: value.address_mode_u.into(),
            address_mode_v: value.address_mode_v.into(),
            address_mode_w: value.address_mode_w.into(),
            mag_filter: value.mag_filter.into(),
            min_filter: value.min_filter.into(),
            mipmap_filter: value.mipmap_filter.into(),
            lod_min_clamp: value.lod_min_clamp,
            lod_max_clamp: value.lod_max_clamp,
            compare: value.compare.map(Into::into),
            anisotropy_clamp: value.anisotropy_clamp,
            border_color: value.border_color.map(Into::into),
        }
    }
}

impl Default for Image {
    /// default is a 1x1x1 all '1.0' texture
    fn default() -> Self {
        let format = wgpu::TextureFormat::bevy_default();
        let data = vec![255; format.pixel_size()];
        Image {
            data,
            texture_descriptor: wgpu::TextureDescriptor {
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                format,
                dimension: wgpu::TextureDimension::D2,
                label: None,
                mip_level_count: 1,
                sample_count: 1,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            sampler: ImageSampler::Default,
            texture_view_descriptor: None,
        }
    }
}

impl Image {
    /// Creates a new image from raw binary data and the corresponding metadata.
    ///
    /// # Panics
    /// Panics if the length of the `data`, volume of the `size` and the size of the `format`
    /// do not match.
    pub fn new(
        size: Extent3d,
        dimension: TextureDimension,
        data: Vec<u8>,
        format: TextureFormat,
    ) -> Self {
        debug_assert_eq!(
            size.volume() * format.pixel_size(),
            data.len(),
            "Pixel data, size and format have to match",
        );
        let mut image = Self {
            data,
            ..Default::default()
        };
        image.texture_descriptor.dimension = dimension;
        image.texture_descriptor.size = size;
        image.texture_descriptor.format = format;
        image
    }

    /// Creates a new image from raw binary data and the corresponding metadata, by filling
    /// the image data with the `pixel` data repeated multiple times.
    ///
    /// # Panics
    /// Panics if the size of the `format` is not a multiple of the length of the `pixel` data.
    pub fn new_fill(
        size: Extent3d,
        dimension: TextureDimension,
        pixel: &[u8],
        format: TextureFormat,
    ) -> Self {
        let mut value = Image::default();
        value.texture_descriptor.format = format;
        value.texture_descriptor.dimension = dimension;
        value.resize(size);

        debug_assert_eq!(
            pixel.len() % format.pixel_size(),
            0,
            "Must not have incomplete pixel data."
        );
        debug_assert!(
            pixel.len() <= value.data.len(),
            "Fill data must fit within pixel buffer."
        );

        for current_pixel in value.data.chunks_exact_mut(pixel.len()) {
            current_pixel.copy_from_slice(pixel);
        }
        value
    }

    /// Returns the width of a 2D image.
    #[inline]
    pub fn width(&self) -> u32 {
        self.texture_descriptor.size.width
    }

    /// Returns the height of a 2D image.
    #[inline]
    pub fn height(&self) -> u32 {
        self.texture_descriptor.size.height
    }

    /// Returns the aspect ratio (height/width) of a 2D image.
    #[inline]
    pub fn aspect_ratio(&self) -> f32 {
        self.height() as f32 / self.width() as f32
    }

    /// Returns the size of a 2D image as f32.
    #[inline]
    pub fn size_f32(&self) -> Vec2 {
        Vec2::new(self.width() as f32, self.height() as f32)
    }

    /// Returns the size of a 2D image.
    #[inline]
    pub fn size(&self) -> UVec2 {
        UVec2::new(self.width(), self.height())
    }

    /// Resizes the image to the new size, by removing information or appending 0 to the `data`.
    /// Does not properly resize the contents of the image, but only its internal `data` buffer.
    pub fn resize(&mut self, size: Extent3d) {
        self.texture_descriptor.size = size;
        self.data.resize(
            size.volume() * self.texture_descriptor.format.pixel_size(),
            0,
        );
    }

    /// Changes the `size`, asserting that the total number of data elements (pixels) remains the
    /// same.
    ///
    /// # Panics
    /// Panics if the `new_size` does not have the same volume as to old one.
    pub fn reinterpret_size(&mut self, new_size: Extent3d) {
        assert!(
            new_size.volume() == self.texture_descriptor.size.volume(),
            "Incompatible sizes: old = {:?} new = {:?}",
            self.texture_descriptor.size,
            new_size
        );

        self.texture_descriptor.size = new_size;
    }

    /// Takes a 2D image containing vertically stacked images of the same size, and reinterprets
    /// it as a 2D array texture, where each of the stacked images becomes one layer of the
    /// array. This is primarily for use with the `texture2DArray` shader uniform type.
    ///
    /// # Panics
    /// Panics if the texture is not 2D, has more than one layers or is not evenly dividable into
    /// the `layers`.
    pub fn reinterpret_stacked_2d_as_array(&mut self, layers: u32) {
        // Must be a stacked image, and the height must be divisible by layers.
        assert!(self.texture_descriptor.dimension == TextureDimension::D2);
        assert!(self.texture_descriptor.size.depth_or_array_layers == 1);
        assert_eq!(self.height() % layers, 0);

        self.reinterpret_size(Extent3d {
            width: self.width(),
            height: self.height() / layers,
            depth_or_array_layers: layers,
        });
    }

    /// Convert a texture from a format to another. Only a few formats are
    /// supported as input and output:
    /// - `TextureFormat::R8Unorm`
    /// - `TextureFormat::Rg8Unorm`
    /// - `TextureFormat::Rgba8UnormSrgb`
    ///
    /// To get [`Image`] as a [`image::DynamicImage`] see:
    /// [`Image::try_into_dynamic`].
    pub fn convert(&self, new_format: TextureFormat) -> Option<Self> {
        self.clone()
            .try_into_dynamic()
            .ok()
            .and_then(|img| match new_format {
                TextureFormat::R8Unorm => {
                    Some((image::DynamicImage::ImageLuma8(img.into_luma8()), false))
                }
                TextureFormat::Rg8Unorm => Some((
                    image::DynamicImage::ImageLumaA8(img.into_luma_alpha8()),
                    false,
                )),
                TextureFormat::Rgba8UnormSrgb => {
                    Some((image::DynamicImage::ImageRgba8(img.into_rgba8()), true))
                }
                _ => None,
            })
            .map(|(dyn_img, is_srgb)| Self::from_dynamic(dyn_img, is_srgb))
    }

    /// Load a bytes buffer in a [`Image`], according to type `image_type`, using the `image`
    /// crate
    pub fn from_buffer(
        buffer: &[u8],
        image_type: ImageType,
        #[allow(unused_variables)] supported_compressed_formats: CompressedImageFormats,
        is_srgb: bool,
        image_sampler: ImageSampler,
    ) -> Result<Image, TextureError> {
        let format = image_type.to_image_format()?;

        // Load the image in the expected format.
        // Some formats like PNG allow for R or RG textures too, so the texture
        // format needs to be determined. For RGB textures an alpha channel
        // needs to be added, so the image data needs to be converted in those
        // cases.

        let mut image = match format {
            #[cfg(feature = "basis-universal")]
            ImageFormat::Basis => {
                basis_buffer_to_image(buffer, supported_compressed_formats, is_srgb)?
            }
            #[cfg(feature = "dds")]
            ImageFormat::Dds => dds_buffer_to_image(buffer, supported_compressed_formats, is_srgb)?,
            #[cfg(feature = "ktx2")]
            ImageFormat::Ktx2 => {
                ktx2_buffer_to_image(buffer, supported_compressed_formats, is_srgb)?
            }
            _ => {
                let image_crate_format = format
                    .as_image_crate_format()
                    .ok_or_else(|| TextureError::UnsupportedTextureFormat(format!("{format:?}")))?;
                let mut reader = image::io::Reader::new(std::io::Cursor::new(buffer));
                reader.set_format(image_crate_format);
                reader.no_limits();
                let dyn_img = reader.decode()?;
                Self::from_dynamic(dyn_img, is_srgb)
            }
        };
        image.sampler = image_sampler;
        Ok(image)
    }

    /// Whether the texture format is compressed or uncompressed
    pub fn is_compressed(&self) -> bool {
        let format_description = self.texture_descriptor.format;
        format_description
            .required_features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC)
            || format_description
                .required_features()
                .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
            || format_description
                .required_features()
                .contains(wgpu::Features::TEXTURE_COMPRESSION_ETC2)
    }

    /// Compute the byte offset where the data of a specific pixel is stored
    ///
    /// Returns None if the provided coordinates are out of bounds.
    ///
    /// For 2D textures, Z is ignored. For 1D textures, Y and Z are ignored.
    #[inline(always)]
    pub fn pixel_data_offset(&self, coords: UVec3) -> Option<usize> {
        let width = self.texture_descriptor.size.width;
        let height = self.texture_descriptor.size.height;
        let depth = self.texture_descriptor.size.depth_or_array_layers;

        let pixel_size = self.texture_descriptor.format.pixel_size();
        let pixel_offset = match self.texture_descriptor.dimension {
            TextureDimension::D3 => {
                if coords.x > width || coords.y > height || coords.z > depth {
                    return None;
                }
                coords.z * height * width + coords.y * width + coords.x
            }
            TextureDimension::D2 => {
                if coords.x > width || coords.y > height {
                    return None;
                }
                coords.y * width + coords.x
            }
            TextureDimension::D1 => {
                if coords.x > width {
                    return None;
                }
                coords.x
            }
        };

        Some(pixel_offset as usize * pixel_size)
    }

    /// Get a reference to the data bytes where a specific pixel's value is stored
    #[inline(always)]
    pub fn pixel_bytes(&self, coords: UVec3) -> Option<&[u8]> {
        let len = self.texture_descriptor.format.pixel_size();
        self.pixel_data_offset(coords)
            .map(|start| &self.data[start..(start + len)])
    }

    /// Get a mutable reference to the data bytes where a specific pixel's value is stored
    #[inline(always)]
    pub fn pixel_bytes_mut(&mut self, coords: UVec3) -> Option<&mut [u8]> {
        let len = self.texture_descriptor.format.pixel_size();
        self.pixel_data_offset(coords)
            .map(|start| &mut self.data[start..(start + len)])
    }

    /// Read the color of a specific pixel.
    ///
    /// This function will find the raw byte data of a specific pixel and
    /// decode it into a user-friendly [`Color`] struct for you.
    ///
    /// Supports many of the common [`TextureFormat`]s:
    ///  - RGBA/BGRA 8-bit unsigned integer, both sRGB and Linear
    ///  - 16-bit and 32-bit unsigned integer
    ///  - 32-bit float
    ///
    /// Be careful: as the data is converted to [`Color`] (which uses `f32` internally),
    /// there may be issues with precision when using non-float [`TextureFormat`]s.
    /// If you read a value you previously wrote using `set_color_at`, it will not match.
    /// If you are working with a 32-bit integer [`TextureFormat`], the value will be
    /// inaccurate (as `f32` does not have enough bits to represent it exactly).
    ///
    /// Single channel (R) formats are assumed to represent greyscale, so the value
    /// will be copied to all three RGB channels in the resulting [`Color`].
    ///
    /// Other [`TextureFormat`]s are unsupported, such as:
    ///  - block-compressed formats
    ///  - non-byte-aligned formats like 10-bit
    ///  - 16-bit float formats
    ///  - signed integer formats
    #[inline(always)]
    pub fn get_color_at(&self, coords: UVec3) -> Result<Color, TextureAccessError> {
        let Some(bytes) = self.pixel_bytes(coords) else {
            return Err(TextureAccessError::OutOfBounds(format!(
                "x: {}, y: {}, z: {}",
                coords.x, coords.y, coords.z
            )));
        };

        match self.texture_descriptor.format {
            TextureFormat::Rgba8UnormSrgb => Ok(Color::rgba(
                bytes[0] as f32 / u8::MAX as f32,
                bytes[1] as f32 / u8::MAX as f32,
                bytes[2] as f32 / u8::MAX as f32,
                bytes[3] as f32 / u8::MAX as f32,
            )),
            TextureFormat::Rgba8Unorm | TextureFormat::Rgba8Uint => Ok(Color::rgba_linear(
                bytes[0] as f32 / u8::MAX as f32,
                bytes[1] as f32 / u8::MAX as f32,
                bytes[2] as f32 / u8::MAX as f32,
                bytes[3] as f32 / u8::MAX as f32,
            )),
            TextureFormat::Bgra8UnormSrgb => Ok(Color::rgba(
                bytes[3] as f32 / u8::MAX as f32,
                bytes[2] as f32 / u8::MAX as f32,
                bytes[1] as f32 / u8::MAX as f32,
                bytes[0] as f32 / u8::MAX as f32,
            )),
            TextureFormat::Bgra8Unorm => Ok(Color::rgba_linear(
                bytes[3] as f32 / u8::MAX as f32,
                bytes[2] as f32 / u8::MAX as f32,
                bytes[1] as f32 / u8::MAX as f32,
                bytes[0] as f32 / u8::MAX as f32,
            )),
            TextureFormat::Rgba32Float => Ok(Color::rgba_linear(
                f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
                f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
                f32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            )),
            TextureFormat::Rgba16Unorm | TextureFormat::Rgba16Uint => {
                let (r, g, b, a) = (
                    u16::from_le_bytes([bytes[0], bytes[1]]),
                    u16::from_le_bytes([bytes[2], bytes[3]]),
                    u16::from_le_bytes([bytes[4], bytes[5]]),
                    u16::from_le_bytes([bytes[6], bytes[7]]),
                );
                Ok(Color::rgba_linear(
                    // going via f64 to avoid rounding errors with large numbers and division
                    (r as f64 / u16::MAX as f64) as f32,
                    (g as f64 / u16::MAX as f64) as f32,
                    (b as f64 / u16::MAX as f64) as f32,
                    (a as f64 / u16::MAX as f64) as f32,
                ))
            }
            TextureFormat::Rgba32Uint => {
                let (r, g, b, a) = (
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                    u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
                    u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
                    u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
                );
                Ok(Color::rgba_linear(
                    // going via f64 to avoid rounding errors with large numbers and division
                    (r as f64 / u32::MAX as f64) as f32,
                    (g as f64 / u32::MAX as f64) as f32,
                    (b as f64 / u32::MAX as f64) as f32,
                    (a as f64 / u32::MAX as f64) as f32,
                ))
            }
            // assume R-only texture format means grayscale (linear)
            // copy value to all of RGB in Color
            TextureFormat::R8Unorm | TextureFormat::R8Uint => {
                let x = bytes[0] as f32 / u8::MAX as f32;
                Ok(Color::rgba_linear(x, x, x, 1.0))
            }
            TextureFormat::R16Unorm | TextureFormat::R16Uint => {
                let x = u16::from_le_bytes([bytes[0], bytes[1]]);
                // going via f64 to avoid rounding errors with large numbers and division
                let x = (x as f64 / u16::MAX as f64) as f32;
                Ok(Color::rgba_linear(x, x, x, 1.0))
            }
            TextureFormat::R32Uint => {
                let x = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                // going via f64 to avoid rounding errors with large numbers and division
                let x = (x as f64 / u32::MAX as f64) as f32;
                Ok(Color::rgba_linear(x, x, x, 1.0))
            }
            TextureFormat::R32Float => {
                let x = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                Ok(Color::rgba_linear(x, x, x, 1.0))
            }
            TextureFormat::Rg8Unorm | TextureFormat::Rg8Uint => {
                let r = bytes[0] as f32 / u8::MAX as f32;
                let g = bytes[1] as f32 / u8::MAX as f32;
                Ok(Color::rgba_linear(r, g, 0.0, 1.0))
            }
            TextureFormat::Rg16Unorm | TextureFormat::Rg16Uint => {
                let r = u16::from_le_bytes([bytes[0], bytes[1]]);
                let g = u16::from_le_bytes([bytes[2], bytes[3]]);
                // going via f64 to avoid rounding errors with large numbers and division
                let r = (r as f64 / u16::MAX as f64) as f32;
                let g = (g as f64 / u16::MAX as f64) as f32;
                Ok(Color::rgba_linear(r, g, 0.0, 1.0))
            }
            TextureFormat::Rg32Uint => {
                let r = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                let g = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                // going via f64 to avoid rounding errors with large numbers and division
                let r = (r as f64 / u32::MAX as f64) as f32;
                let g = (g as f64 / u32::MAX as f64) as f32;
                Ok(Color::rgba_linear(r, g, 0.0, 1.0))
            }
            TextureFormat::Rg32Float => {
                let r = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                let g = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                Ok(Color::rgba_linear(r, g, 0.0, 1.0))
            }
            _ => {
                return Err(TextureAccessError::UnsupportedTextureFormat(format!(
                    "{:?}",
                    self.texture_descriptor.format
                )));
            }
        }
    }

    /// Change the color of a specific pixel.
    ///
    /// This function will find the raw byte data of a specific pixel and
    /// change it according to a [`Color`] you provide. The [`Color`] struct
    /// will be encoded into the [`Image`]'s [`TextureFormat`].
    ///
    /// Supports many of the common [`TextureFormat`]s:
    ///  - RGBA/BGRA 8-bit unsigned integer, both sRGB and Linear
    ///  - 16-bit and 32-bit unsigned integer (with possibly-limited precision, as [`Color`] uses `f32`)
    ///  - 32-bit float
    ///
    /// Be careful: writing to non-float [`TextureFormat`]s is lossy! The data has to be converted,
    /// so if you read it back using `get_color_at`, the `Color` you get will not equal the value
    /// you used when writing it using this function.
    ///
    /// For R and RG formats, only the respective values from the linear RGB [`Color`] will be used.
    ///
    /// Other [`TextureFormat`]s are unsupported, such as:
    ///  - block-compressed formats
    ///  - non-byte-aligned formats like 10-bit
    ///  - 16-bit float formats
    ///  - signed integer formats
    #[inline(always)]
    pub fn set_color_at(&mut self, coords: UVec3, color: Color) -> Result<(), TextureAccessError> {
        let format = self.texture_descriptor.format;

        let Some(bytes) = self.pixel_bytes_mut(coords) else {
            return Err(TextureAccessError::OutOfBounds(format!(
                "x: {}, y: {}, z: {}",
                coords.x, coords.y, coords.z
            )));
        };

        match format {
            TextureFormat::Rgba8UnormSrgb => {
                let [r, g, b, a] = color.as_rgba_f32();
                bytes[0] = (r * u8::MAX as f32) as u8;
                bytes[1] = (g * u8::MAX as f32) as u8;
                bytes[2] = (b * u8::MAX as f32) as u8;
                bytes[3] = (a * u8::MAX as f32) as u8;
            }
            TextureFormat::Rgba8Unorm | TextureFormat::Rgba8Uint => {
                let [r, g, b, a] = color.as_linear_rgba_f32();
                bytes[0] = (r * u8::MAX as f32) as u8;
                bytes[1] = (g * u8::MAX as f32) as u8;
                bytes[2] = (b * u8::MAX as f32) as u8;
                bytes[3] = (a * u8::MAX as f32) as u8;
            }
            TextureFormat::Bgra8UnormSrgb => {
                let [r, g, b, a] = color.as_rgba_f32();
                bytes[0] = (b * u8::MAX as f32) as u8;
                bytes[1] = (g * u8::MAX as f32) as u8;
                bytes[2] = (r * u8::MAX as f32) as u8;
                bytes[3] = (a * u8::MAX as f32) as u8;
            }
            TextureFormat::Bgra8Unorm => {
                let [r, g, b, a] = color.as_linear_rgba_f32();
                bytes[0] = (b * u8::MAX as f32) as u8;
                bytes[1] = (g * u8::MAX as f32) as u8;
                bytes[2] = (r * u8::MAX as f32) as u8;
                bytes[3] = (a * u8::MAX as f32) as u8;
            }
            TextureFormat::Rgba32Float => {
                let [r, g, b, a] = color.as_linear_rgba_f32();
                bytes[0..4].copy_from_slice(&f32::to_le_bytes(r));
                bytes[4..8].copy_from_slice(&f32::to_le_bytes(g));
                bytes[8..12].copy_from_slice(&f32::to_le_bytes(b));
                bytes[12..16].copy_from_slice(&f32::to_le_bytes(a));
            }
            TextureFormat::Rgba16Unorm | TextureFormat::Rgba16Uint => {
                let [r, g, b, a] = color.as_linear_rgba_f32();
                let [r, g, b, a] = [
                    (r * u16::MAX as f32) as u16,
                    (g * u16::MAX as f32) as u16,
                    (b * u16::MAX as f32) as u16,
                    (a * u16::MAX as f32) as u16,
                ];
                bytes[0..2].copy_from_slice(&u16::to_le_bytes(r));
                bytes[2..4].copy_from_slice(&u16::to_le_bytes(g));
                bytes[4..6].copy_from_slice(&u16::to_le_bytes(b));
                bytes[6..8].copy_from_slice(&u16::to_le_bytes(a));
            }
            TextureFormat::Rgba32Uint => {
                let [r, g, b, a] = color.as_linear_rgba_f32();
                let [r, g, b, a] = [
                    (r * u32::MAX as f32) as u32,
                    (g * u32::MAX as f32) as u32,
                    (b * u32::MAX as f32) as u32,
                    (a * u32::MAX as f32) as u32,
                ];
                bytes[0..4].copy_from_slice(&u32::to_le_bytes(r));
                bytes[4..8].copy_from_slice(&u32::to_le_bytes(g));
                bytes[8..12].copy_from_slice(&u32::to_le_bytes(b));
                bytes[12..16].copy_from_slice(&u32::to_le_bytes(a));
            }
            TextureFormat::R8Unorm | TextureFormat::R8Uint => {
                // TODO: this should probably be changed to do
                // a proper conversion into greyscale
                let [r, _, _, _] = color.as_linear_rgba_f32();
                bytes[0] = (r * u8::MAX as f32) as u8;
            }
            TextureFormat::R16Unorm | TextureFormat::R16Uint => {
                // TODO: this should probably be changed to do
                // a proper conversion into greyscale
                let [r, _, _, _] = color.as_linear_rgba_f32();
                let r = (r * u16::MAX as f32) as u16;
                bytes[0..2].copy_from_slice(&u16::to_le_bytes(r));
            }
            TextureFormat::R32Uint => {
                // TODO: this should probably be changed to do
                // a proper conversion into greyscale
                let [r, _, _, _] = color.as_linear_rgba_f32();
                // go via f64 to avoid imprecision
                let r = (r as f64 * u32::MAX as f64) as u32;
                bytes[0..4].copy_from_slice(&u32::to_le_bytes(r));
            }
            TextureFormat::R32Float => {
                // TODO: this should probably be changed to do
                // a proper conversion into greyscale
                let [r, _, _, _] = color.as_linear_rgba_f32();
                bytes[0..4].copy_from_slice(&f32::to_le_bytes(r));
            }
            TextureFormat::Rg8Unorm | TextureFormat::Rg8Uint => {
                let [r, g, _, _] = color.as_linear_rgba_f32();
                bytes[0] = (r * u8::MAX as f32) as u8;
                bytes[1] = (g * u8::MAX as f32) as u8;
            }
            TextureFormat::Rg16Unorm | TextureFormat::Rg16Uint => {
                let [r, g, _, _] = color.as_linear_rgba_f32();
                let r = (r * u16::MAX as f32) as u16;
                let g = (g * u16::MAX as f32) as u16;
                bytes[0..2].copy_from_slice(&u16::to_le_bytes(r));
                bytes[2..4].copy_from_slice(&u16::to_le_bytes(g));
            }
            TextureFormat::Rg32Uint => {
                let [r, g, _, _] = color.as_linear_rgba_f32();
                // go via f64 to avoid imprecision
                let r = (r as f64 * u32::MAX as f64) as u32;
                let g = (g as f64 * u32::MAX as f64) as u32;
                bytes[0..4].copy_from_slice(&u32::to_le_bytes(r));
                bytes[4..8].copy_from_slice(&u32::to_le_bytes(g));
            }
            TextureFormat::Rg32Float => {
                let [r, g, _, _] = color.as_linear_rgba_f32();
                bytes[0..4].copy_from_slice(&f32::to_le_bytes(r));
                bytes[4..8].copy_from_slice(&f32::to_le_bytes(g));
            }
            _ => {
                return Err(TextureAccessError::UnsupportedTextureFormat(format!(
                    "{:?}",
                    self.texture_descriptor.format
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DataFormat {
    Rgb,
    Rgba,
    Rrr,
    Rrrg,
    Rg,
}

#[derive(Clone, Copy, Debug)]
pub enum TranscodeFormat {
    Etc1s,
    Uastc(DataFormat),
    // Has to be transcoded to R8Unorm for use with `wgpu`
    R8UnormSrgb,
    // Has to be transcoded to R8G8Unorm for use with `wgpu`
    Rg8UnormSrgb,
    // Has to be transcoded to Rgba8 for use with `wgpu`
    Rgb8,
}

/// An error that occurs when accessing specific pixels in a texture
#[derive(Error, Debug)]
pub enum TextureAccessError {
    #[error("out of bounds ({0})")]
    OutOfBounds(String),
    #[error("unsupported texture format: {0}")]
    UnsupportedTextureFormat(String),
}

/// An error that occurs when loading a texture
#[derive(Error, Debug)]
pub enum TextureError {
    #[error("invalid image mime type: {0}")]
    InvalidImageMimeType(String),
    #[error("invalid image extension: {0}")]
    InvalidImageExtension(String),
    #[error("failed to load an image: {0}")]
    ImageError(#[from] image::ImageError),
    #[error("unsupported texture format: {0}")]
    UnsupportedTextureFormat(String),
    #[error("supercompression not supported: {0}")]
    SuperCompressionNotSupported(String),
    #[error("failed to load an image: {0}")]
    SuperDecompressionError(String),
    #[error("invalid data: {0}")]
    InvalidData(String),
    #[error("transcode error: {0}")]
    TranscodeError(String),
    #[error("format requires transcoding: {0:?}")]
    FormatRequiresTranscodingError(TranscodeFormat),
    /// Only cubemaps with six faces are supported.
    #[error("only cubemaps with six faces are supported")]
    IncompleteCubemap,
}

/// The type of a raw image buffer.
#[derive(Debug)]
pub enum ImageType<'a> {
    /// The mime type of an image, for example `"image/png"`.
    MimeType(&'a str),
    /// The extension of an image file, for example `"png"`.
    Extension(&'a str),
    /// The direct format of the image
    Format(ImageFormat),
}

impl<'a> ImageType<'a> {
    pub fn to_image_format(&self) -> Result<ImageFormat, TextureError> {
        match self {
            ImageType::MimeType(mime_type) => ImageFormat::from_mime_type(mime_type)
                .ok_or_else(|| TextureError::InvalidImageMimeType(mime_type.to_string())),
            ImageType::Extension(extension) => ImageFormat::from_extension(extension)
                .ok_or_else(|| TextureError::InvalidImageExtension(extension.to_string())),
            ImageType::Format(format) => Ok(*format),
        }
    }
}

/// Used to calculate the volume of an item.
pub trait Volume {
    fn volume(&self) -> usize;
}

impl Volume for Extent3d {
    /// Calculates the volume of the [`Extent3d`].
    fn volume(&self) -> usize {
        (self.width * self.height * self.depth_or_array_layers) as usize
    }
}

/// Extends the wgpu [`TextureFormat`] with information about the pixel.
pub trait TextureFormatPixelInfo {
    /// Returns the size of a pixel in bytes of the format.
    fn pixel_size(&self) -> usize;
}

impl TextureFormatPixelInfo for TextureFormat {
    fn pixel_size(&self) -> usize {
        let info = self;
        match info.block_dimensions() {
            (1, 1) => info.block_size(None).unwrap() as usize,
            _ => panic!("Using pixel_size for compressed textures is invalid"),
        }
    }
}

/// The GPU-representation of an [`Image`].
/// Consists of the [`Texture`], its [`TextureView`] and the corresponding [`Sampler`], and the texture's size.
#[derive(Debug, Clone)]
pub struct GpuImage {
    pub texture: Texture,
    pub texture_view: TextureView,
    pub texture_format: TextureFormat,
    pub sampler: Sampler,
    pub size: Vec2,
    pub mip_level_count: u32,
}

impl RenderAsset for Image {
    type ExtractedAsset = Image;
    type PreparedAsset = GpuImage;
    type Param = (
        SRes<RenderDevice>,
        SRes<RenderQueue>,
        SRes<DefaultImageSampler>,
    );

    /// Clones the Image.
    fn extract_asset(&self) -> Self::ExtractedAsset {
        self.clone()
    }

    /// Converts the extracted image into a [`GpuImage`].
    fn prepare_asset(
        image: Self::ExtractedAsset,
        (render_device, render_queue, default_sampler): &mut SystemParamItem<Self::Param>,
    ) -> Result<Self::PreparedAsset, PrepareAssetError<Self::ExtractedAsset>> {
        let texture = render_device.create_texture_with_data(
            render_queue,
            &image.texture_descriptor,
            &image.data,
        );

        let texture_view = texture.create_view(
            image
                .texture_view_descriptor
                .or_else(|| Some(TextureViewDescriptor::default()))
                .as_ref()
                .unwrap(),
        );
        let size = Vec2::new(
            image.texture_descriptor.size.width as f32,
            image.texture_descriptor.size.height as f32,
        );
        let sampler = match image.sampler {
            ImageSampler::Default => (***default_sampler).clone(),
            ImageSampler::Descriptor(descriptor) => {
                render_device.create_sampler(&descriptor.as_wgpu())
            }
        };

        Ok(GpuImage {
            texture,
            texture_view,
            texture_format: image.texture_descriptor.format,
            sampler,
            size,
            mip_level_count: image.texture_descriptor.mip_level_count,
        })
    }
}

bitflags::bitflags! {
    #[derive(Default, Clone, Copy, Eq, PartialEq, Debug)]
    #[repr(transparent)]
    pub struct CompressedImageFormats: u32 {
        const NONE     = 0;
        const ASTC_LDR = (1 << 0);
        const BC       = (1 << 1);
        const ETC2     = (1 << 2);
    }
}

impl CompressedImageFormats {
    pub fn from_features(features: wgpu::Features) -> Self {
        let mut supported_compressed_formats = Self::default();
        if features.contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC) {
            supported_compressed_formats |= Self::ASTC_LDR;
        }
        if features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
            supported_compressed_formats |= Self::BC;
        }
        if features.contains(wgpu::Features::TEXTURE_COMPRESSION_ETC2) {
            supported_compressed_formats |= Self::ETC2;
        }
        supported_compressed_formats
    }

    pub fn supports(&self, format: TextureFormat) -> bool {
        match format {
            TextureFormat::Bc1RgbaUnorm
            | TextureFormat::Bc1RgbaUnormSrgb
            | TextureFormat::Bc2RgbaUnorm
            | TextureFormat::Bc2RgbaUnormSrgb
            | TextureFormat::Bc3RgbaUnorm
            | TextureFormat::Bc3RgbaUnormSrgb
            | TextureFormat::Bc4RUnorm
            | TextureFormat::Bc4RSnorm
            | TextureFormat::Bc5RgUnorm
            | TextureFormat::Bc5RgSnorm
            | TextureFormat::Bc6hRgbUfloat
            | TextureFormat::Bc6hRgbFloat
            | TextureFormat::Bc7RgbaUnorm
            | TextureFormat::Bc7RgbaUnormSrgb => self.contains(CompressedImageFormats::BC),
            TextureFormat::Etc2Rgb8Unorm
            | TextureFormat::Etc2Rgb8UnormSrgb
            | TextureFormat::Etc2Rgb8A1Unorm
            | TextureFormat::Etc2Rgb8A1UnormSrgb
            | TextureFormat::Etc2Rgba8Unorm
            | TextureFormat::Etc2Rgba8UnormSrgb
            | TextureFormat::EacR11Unorm
            | TextureFormat::EacR11Snorm
            | TextureFormat::EacRg11Unorm
            | TextureFormat::EacRg11Snorm => self.contains(CompressedImageFormats::ETC2),
            TextureFormat::Astc { .. } => self.contains(CompressedImageFormats::ASTC_LDR),
            _ => true,
        }
    }
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn image_size() {
        let size = Extent3d {
            width: 200,
            height: 100,
            depth_or_array_layers: 1,
        };
        let image = Image::new_fill(
            size,
            TextureDimension::D2,
            &[0, 0, 0, 255],
            TextureFormat::Rgba8Unorm,
        );
        assert_eq!(
            Vec2::new(size.width as f32, size.height as f32),
            image.size_f32()
        );
    }
    #[test]
    fn image_default_size() {
        let image = Image::default();
        assert_eq!(Vec2::ONE, image.size_f32());
    }
}
