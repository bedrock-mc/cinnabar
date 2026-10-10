#!/usr/bin/env python3
"""Render the original OreUI assets without reading any installed game files."""

import argparse
import json
from pathlib import Path
import tempfile

from PIL import Image
from animations import highlight_sheet, loading_frames, ping_frame, ping_sheet
from icons import icon
from illustrations import overworld_block, pack_cover, vignette
from scenes import scene

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / 'assets/oreui'


def load_manifest():
    """Read the public replacement contract that owns filenames and dimensions."""
    return json.loads((ASSETS/'manifest.json').read_text())


def draw(entry):
    """Render one manifest recipe as a list of original RGBA images."""
    family,name = entry['recipe'].split(':',1)
    if family == 'icon':
        result = icon(name)
    elif family == 'scene':
        result = scene(name,tuple(entry['size']))
    elif family == 'vignette':
        result = vignette(name)
    elif family == 'ping':
        result = ping_frame(int(name))
    elif family == 'illustration':
        result = overworld_block() if name=='overworld-block' else pack_cover(name=='missing-pack')
    elif family == 'animation':
        if name == 'loading':
            return loading_frames()
        result = {'ping':ping_sheet,'highlight':highlight_sheet}[name]()
    else:
        raise ValueError(f'Unknown recipe: {entry["recipe"]}')
    return [result]


def gif_palette(frames):
    """Index only the authored opaque colours and reserve index zero for transparency."""
    colors = sorted({pixel[:3] for frame in frames for pixel in frame.get_flattened_data() if pixel[3]})
    assert len(colors) < 256
    palette = [(0,0,0)] + colors
    indices = {color:i+1 for i,color in enumerate(colors)}
    flat = [channel for color in palette for channel in color]
    flat += [0]*(768-len(flat))
    indexed = []
    for frame in frames:
        image = Image.new('P',frame.size)
        image.putpalette(flat)
        image.putdata([indices[pixel[:3]] if pixel[3] else 0 for pixel in frame.get_flattened_data()])
        indexed.append(image)
    return indexed


def write_asset(entry, frames, destination):
    """Encode a PNG or transparent GIF and check the declared pixel contract."""
    expected = tuple(entry['size'])
    assert all(frame.size==expected for frame in frames),entry['id']
    path = destination/entry['file']
    if path.suffix == '.gif':
        assert len(frames)==entry['frames'],entry['id']
        indexed = gif_palette(frames)
        indexed[0].save(path,save_all=True,append_images=indexed[1:],
                        duration=entry['playback']['durations_ms'],
                        loop=entry['playback']['loop'],transparency=0,
                        disposal=2,optimize=False)
    else:
        frames[0].save(path,optimize=False,compress_level=9)
    with Image.open(path) as image:
        assert image.size == expected
        if path.suffix == '.gif':
            assert image.n_frames==entry['frames']
            durations=[]
            for i in range(image.n_frames):
                image.seek(i)
                durations.append(image.info['duration'])
            assert durations==entry['playback']['durations_ms']


def render_all(destination):
    """Render every replacement to the requested directory from public source alone."""
    destination.mkdir(parents=True,exist_ok=True)
    entries = load_manifest()['assets']
    assert len({e['game_key'] for e in entries})==len(entries)
    assert len({e['file'] for e in entries})==len(entries)
    for entry in entries:
        write_asset(entry,draw(entry),destination)
    return entries


def main():
    """Regenerate outputs or compare a temporary regeneration with committed assets."""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ASSETS)
    parser.add_argument('--check',action='store_true')
    args=parser.parse_args()
    if args.check:
        with tempfile.TemporaryDirectory(prefix='oreui-art-check-') as directory:
            destination=Path(directory)
            entries=render_all(destination)
            changed=[e['file'] for e in entries if not (args.output/e['file']).exists()
                     or (destination/e['file']).read_bytes()!=(args.output/e['file']).read_bytes()]
            if changed:
                raise SystemExit('Regenerate these assets: '+', '.join(changed))
        print(f'{len(entries)} assets reproduce byte for byte; dimensions and GIF timing checked.')
    else:
        entries=render_all(args.output)
        print(f'Rendered {len(entries)} original assets to {args.output}')


if __name__=='__main__':
    main()
