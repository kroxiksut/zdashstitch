"""Tiny reference conversion for colors; production must use versioned Rust WidgetCodec."""
import re

def graph_color_to_zabbix(color: str) -> str:
    if not isinstance(color, str) or not re.fullmatch(r'#[0-9a-fA-F]{6}', color):
        raise ValueError('Invalid logical RGB color')
    return color[1:].upper()
