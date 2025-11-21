"""Simple SVG to PIL converter using cairosvg"""
import os
import io
import cairosvg
from PIL import Image
from loguru import logger as log


def svg_to_pil(svg_path: str, size: tuple = (512, 512)) -> 'Image.Image | None':
    """
    Convert SVG file to PIL Image using cairosvg with proper scaling and cropping
    
    Args:
        svg_path: Path to SVG file
        size: Target size (width, height)
        
    Returns:
        PIL Image or None if failed
    """
    if not os.path.exists(svg_path):
        log.error(f"SVG file not found: {svg_path}")
        return None
    
    try:
        with open(svg_path, 'r', encoding='utf-8') as f:
            svg_content = f.read()
        
        png_data = cairosvg.svg2png(
            bytestring=svg_content.encode('utf-8'),
            output_width=size[0],
            output_height=size[1],
            background_color='transparent'
        )
        
        image = Image.open(io.BytesIO(png_data))
        
        if image.mode != 'RGBA':
            image = image.convert('RGBA')
        
        image = _crop_and_pad(image, padding=2)
        
        return image
        
    except Exception as e:
        log.error(f"Error converting SVG to PIL: {e}")
        log.error(f"SVG path: {svg_path}")
        return None


def _crop_and_pad(image: 'Image.Image', padding: int = 2) -> 'Image.Image':
    """
    Crop transparent edges from image and add padding, aligning content to bottom
    
    Args:
        image: PIL Image to crop
        padding: Number of transparent pixels to add around edges
        
    Returns:
        Cropped and padded PIL Image with content aligned to bottom
    """
    try:
        bbox = image.getbbox()
        
        if bbox is None:
            return Image.new('RGBA', (1, 1), (0, 0, 0, 0))
        
        cropped = image.crop(bbox)
        
        width, height = cropped.size
        padded_width = width + (padding * 2)
        padded_height = height + (padding * 2)
        
        padded_image = Image.new('RGBA', (padded_width, padded_height), (0, 0, 0, 0))
        
        bottom_y = padded_height - height - padding
        padded_image.paste(cropped, (padding, bottom_y))
        
        return padded_image
        
    except Exception as e:
        if log:
            log.error(f"Error cropping and padding image: {e}")
        return image
def is_svg_file(file_path: str) -> bool:
    """Check if file is an SVG"""
    return file_path.lower().endswith('.svg')
