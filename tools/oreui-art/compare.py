#!/usr/bin/env python3
"""Build a local-only side-by-side review sheet using an installed image bundle."""

import argparse
import base64
import html
from io import BytesIO
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont
from render import ASSETS, ROOT, load_manifest

PAGE_WIDTH = 1600
PAGE_HEIGHT = 2200


def frames_from(path, entry):
    """Decode a GIF or split a horizontal sheet into its declared storage cells."""
    with Image.open(path) as image:
        assert image.size == tuple(entry['size']),f'Wrong dimensions: {path}'
        if image.format == 'GIF':
            frames,durations = [],[]
            assert image.n_frames == entry['frames'],f'Wrong frame count: {path}'
            for i in range(image.n_frames):
                image.seek(i)
                frames.append(image.convert('RGBA'))
                durations.append(image.info['duration'])
            assert durations == entry['playback']['durations_ms'],f'Wrong timing: {path}'
            assert image.info.get('loop') == entry['playback']['loop']
            return frames
        image = image.convert('RGBA')
        if entry['frames'] == 1:
            return [image]
        width,height = entry['frame_size']
        assert width*entry['frames'] == image.width and height == image.height
        return [image.crop((i*width,0,(i+1)*width,height)) for i in range(entry['frames'])]


def frame_strip(frames):
    """Place every frame in playback order with one transparent separator pixel."""
    width,height = frames[0].size
    strip=Image.new('RGBA',((width+1)*len(frames)-1,height))
    for i,frame in enumerate(frames):
        strip.alpha_composite(frame,(i*(width+1),0))
    return strip


def data_uri(image):
    """Embed a review image as a self-contained PNG data URI."""
    buffer=BytesIO()
    image.save(buffer,format='PNG')
    return 'data:image/png;base64,'+base64.b64encode(buffer.getvalue()).decode('ascii')


def timing_label(entry):
    """Describe stored cells and playback timing without hiding the terminal cell."""
    if entry['frames']==1:
        return '1 still'
    playback=entry['playback']
    if playback['kind']=='gif':
        return f"{entry['frames']} frames · {sum(playback['durations_ms'])} ms loop"
    ending='loop' if playback['repeat']=='infinite' else 'hold last cell'
    return (f"{entry['frames']} cells · {playback['steps']} steps / "
            f"{playback['duration_ms']} ms · {ending}")


def font(size):
    """Load a local review font, falling back to Pillow's portable bundled font."""
    for path in ('/System/Library/Fonts/Menlo.ttc','/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf'):
        if Path(path).exists():
            return ImageFont.truetype(path,size)
    return ImageFont.load_default(size=size)


def check_destination(destination):
    """Refuse comparison output anywhere inside a Git checkout or input asset directory."""
    destination=destination.resolve()
    if destination==ROOT or ROOT in destination.parents:
        raise ValueError('Comparison sheets contain reference art and must stay outside the repository.')
    for ancestor in (destination,*destination.parents):
        if (ancestor/'.git').exists():
            raise ValueError('Comparison sheets must not be written inside any Git checkout.')
    return destination


def html_sheet(records, destination):
    """Write all pairs and all animation cells into one offline, self-contained HTML page."""
    sections=[]
    for entry,original,ours in records:
        width,height=entry['size']
        strips=[frame_strip(original),frame_strip(ours)]
        base_scale=min(3,650/strips[0].width,270/strips[0].height)
        dimensions=f'{width} × {height} px'
        if entry['frames']>1:
            dimensions+=f" · {entry['frame_size'][0]} × {entry['frame_size'][1]} px / cell"
        pictures=[]
        for label,strip in zip(('Installed reference — local review only','Original Cinnabar drawing'),strips):
            pictures.append(f'<figure><figcaption>{label}</figcaption><div class="viewport"><img '
                            f'alt="{html.escape(entry["id"])} — {label}" '
                            f'data-w="{strip.width}" data-h="{strip.height}" '
                            f'data-fit="{base_scale}" width="{round(strip.width*base_scale)}" '
                            f'height="{round(strip.height*base_scale)}" src="{data_uri(strip)}"></div></figure>')
        indices=''
        if entry['frames']>1:
            indices='<p class="meta">Cells left to right: '+', '.join(str(i) for i in range(entry['frames']))+'.</p>'
        sections.append(f'<section data-group="{html.escape(entry["group"])}"><h2>{html.escape(entry["id"])}</h2>'
                        f'<code>{html.escape(entry["game_key"])}</code>'
                        f'<p class="meta">{dimensions} · {timing_label(entry)} · assets/oreui/{entry["file"]}</p>'
                        f'<div class="pair">{"".join(pictures)}</div>{indices}</section>')
    page='''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>OreUI original art — private comparison</title>
<style>
:root {color-scheme:dark;font:15px system-ui;background:#181f28;color:#e3e9ec}
body {margin:0;padding:28px;max-width:1500px;margin-inline:auto}
h1 {font-size:30px;margin-bottom:8px} h2 {font-size:19px;margin:0 0 8px}
header {padding-bottom:24px} header p {max-width:100ch;line-height:1.6}
.controls {position:sticky;top:0;z-index:1;background:#181f28;padding:12px 0;display:flex;gap:20px;flex-wrap:wrap}
select,input {font:inherit;padding:6px;border:1px solid #8294a3;background:#28343f;color:white}
section {padding:22px 0;border-top:1px solid #42515f} code {overflow-wrap:anywhere;color:#a4b7c6}
.meta {font-size:13px;color:#a4b7c6} .pair {display:grid;grid-template-columns:1fr 1fr;gap:18px}
figure {margin:0;min-width:0} figcaption {font-size:13px;padding:0 0 9px}
.viewport {overflow:auto;min-height:95px;padding:20px;display:flex;align-items:center;background-color:#303943;
background-image:linear-gradient(45deg,#35404b 25%,transparent 25%,transparent 75%,#35404b 75%),linear-gradient(45deg,#35404b 25%,transparent 25%,transparent 75%,#35404b 75%);background-size:16px 16px;background-position:0 0,8px 8px}
img {image-rendering:pixelated;max-width:none;flex-shrink:0} [hidden] {display:none!important}
@media(max-width:700px){body{padding:16px}.pair{grid-template-columns:1fr}}
</style><header><h1>OreUI · original artwork</h1>
<p>REPLACEMENT_COUNT replacements, authored with reviewable drawing code. Installed images appear only in this local review file.
Every pair has identical display dimensions and nearest-neighbour scaling. Animations show every stored cell,
including terminal cells. No client integration is included.</p>
<p>The reference bundle uses seven ping cells with six steps over 700 ms, and nine highlight cells with eight
steps over 500 ms. Current client callers differ; the manifest preserves the reference contracts.</p></header>
<div class="controls"><label>Scale <select id="scale"><option value="fit">Fit pairs</option><option value="1">1× native</option><option value="2">2× GUI</option><option value="3">3× GUI</option></select></label>
<label>Find <input id="search" type="search" placeholder="Image name or game key"></label>
<span id="count">REPLACEMENT_COUNT images</span></div>
'''+''.join(sections)+'''
<script>
// Resize both columns from native dimensions so comparisons always use one scale.
function resizePairs(){const value=document.querySelector('#scale').value;for(const img of document.images){const scale=value==='fit'?Number(img.dataset.fit):Number(value);img.width=Math.round(Number(img.dataset.w)*scale);img.height=Math.round(Number(img.dataset.h)*scale);}}
// Filter whole pairs, retaining dimensions and labels for each result.
function filterPairs(){const query=document.querySelector('#search').value.toLowerCase();let count=0;for(const section of document.querySelectorAll('section')){section.hidden=!section.textContent.toLowerCase().includes(query);if(!section.hidden)count++;}document.querySelector('#count').textContent=count+' images';}
document.querySelector('#scale').addEventListener('change',resizePairs);
document.querySelector('#search').addEventListener('input',filterPairs);
</script></html>'''
    (destination/'sheet.html').write_text(page.replace('REPLACEMENT_COUNT',str(len(records))))


def checker(image, box):
    """Paint a muted checkerboard behind transparent review artwork."""
    draw=ImageDraw.Draw(image)
    x0,y0,x1,y1=box
    for y in range(y0,y1,12):
        for x in range(x0,x1,12):
            shade='#303943' if ((x-x0)//12+(y-y0)//12)%2 else '#35404b'
            draw.rectangle((x,y,min(x+11,x1-1),min(y+11,y1-1)),fill=shade)


def png_pages(records, destination):
    """Lay out compact pairs in two columns and landscapes across the whole page."""
    pages=[]
    page=None
    y=PAGE_HEIGHT
    column=0
    row_height=0
    ordered=sorted(records,key=lambda record: record[0]['recipe'].startswith('scene:'))
    for entry,original,ours in ordered:
        large=entry['recipe'].startswith('scene:')
        if large and column:
            y+=row_height
            column=0
            row_height=0
        strips=[frame_strip(original),frame_strip(ours)]
        scale=min(3,(716 if large else 350)/strips[0].width,
                  (360 if large else 156)/strips[0].height)
        width,height=round(strips[0].width*scale),round(strips[0].height*scale)
        card_height=max(height,72)+121
        # A new row must fit both cards, including a taller right-hand card.
        needed=max(card_height,row_height)
        if y+needed>PAGE_HEIGHT-45:
            page=Image.new('RGB',(PAGE_WIDTH,PAGE_HEIGHT),'#181f28')
            pages.append(page)
            d=ImageDraw.Draw(page)
            d.text((40,25),'OREUI / ORIGINAL ART / LOCAL COMPARISON',font=font(25),fill='#f1eee0')
            d.text((40,64),f'Page {len(pages)} · Reference pixels are for private review only',font=font(16),fill='#b0c0cb')
            y=110
            column=0
            row_height=0
        left=40+column*780
        d=ImageDraw.Draw(page)
        d.text((left,y),entry['id'],font=font(16),fill='#e3e9ec')
        d.text((left,y+23),entry['game_key'],font=font(11),fill='#a4b7c6')
        dims=' × '.join(str(n) for n in entry['size'])
        d.text((left,y+43),f"{dims} px · {timing_label(entry)}",font=font(12),fill='#a4b7c6')
        spacing=780 if large else 380
        box_width=736 if large else 356
        for x,label,strip in zip((left,left+spacing),('INSTALLED REFERENCE','ORIGINAL CINNABAR'),strips):
            d.text((x,y+64),label,font=font(11),fill='#c8d5dc')
            checker(page,(x,y+84,x+box_width,y+84+max(height,72)+12))
            scaled=strip.resize((width,height),Image.Resampling.NEAREST)
            page.paste(scaled,(x+(box_width-width)//2,y+90),scaled)
        row_height=max(row_height,card_height)
        if large or column==1:
            y+=row_height
            row_height=0
            column=0
        else:
            column=1
    for i,page in enumerate(pages,1):
        page.save(destination/f'sheet-{i:02}.png',optimize=True)
    # Only remove older pages that this same tool names, leaving other evidence alone.
    for path in destination.glob('sheet-*.png'):
        suffix=path.stem.removeprefix('sheet-')
        if suffix.isdigit() and int(suffix)>len(pages):
            path.unlink()
    return len(pages)


def main():
    """Read the installed bundle only for comparison and write outside every checkout."""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle',required=True,type=Path,help='The directory containing assets/')
    parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args()
    destination=check_destination(args.output)
    bundle=args.bundle.resolve()
    if destination==bundle or bundle in destination.parents or destination in bundle.parents:
        raise ValueError('Keep comparison outputs separate from the installed bundle.')
    records=[]
    for entry in load_manifest()['assets']:
        records.append((entry,frames_from(bundle/entry['game_key'],entry),frames_from(ASSETS/entry['file'],entry)))
    destination.mkdir(parents=True,exist_ok=True)
    html_sheet(records,destination)
    count=png_pages(records,destination)
    print(f'Validated {len(records)} pairs; wrote sheet.html and {count} PNG pages to {destination}')


if __name__=='__main__':
    main()
