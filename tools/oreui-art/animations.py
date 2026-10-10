"""Original loading and UI accent animations; timing comes from the manifest."""

from pixels import PixelArt, rgba


def ping_frame(active, width=20, pending=False):
    """Draw three rising signal bars, lighting only the requested count."""
    art = PixelArt(width,20)
    left = (width-13)//2
    for i in range(3):
        x = left+i*5
        top = 13-i*4
        shade = ('mist' if pending else ('red','gold','leaf')[active-1]) if i<active else 'shadow'
        art.rect((x,top,x+2,16),shade)
        if i<active:
            art.rect((x,top,x+2,top),'cream' if active==2 else 'white')
    return art.image


def ping_sheet():
    """Draw six loop cells plus the terminal repeat cell in a 112×20 sheet."""
    art = PixelArt(112,20)
    for i,active in enumerate((0,1,2,3,2,1,0)):
        art.paste(ping_frame(active,16,pending=True),(i*16,0))
    return art.image


def highlight_sheet():
    """Draw an expanding corner flash that clears after eight transition steps."""
    art = PixelArt(216,24)
    for frame in range(8):
        cell = PixelArt(24,24)
        inset = max(0,6-frame)
        alpha = (210,230,230,210,180,140,90,40)[frame]
        shade = (*rgba('white')[:3],alpha)
        low,high = inset,23-inset
        length = 5 if frame<5 else 3
        for x,dx in ((low,1),(high,-1)):
            for y,dy in ((low,1),(high,-1)):
                cell.line([(x+dx*length,y),(x,y),(x,y+dy*length)],shade)
        art.paste(cell.image,(24*frame,0))
    return art.image


def loading_frames():
    """Draw ten distinct steps of a circulating square-bead loader."""
    points = [(9,3),(16,3),(22,7),(23,14),(19,21),
              (12,23),(5,21),(1,14),(2,7),(5,3)]
    frames = []
    for step in range(10):
        art = PixelArt(28,28)
        for i,(x,y) in enumerate(points):
            age = (step-i)%10
            shade = 'white' if age==0 else 'mist' if age==1 else 'slate' if age<4 else 'shadow'
            art.rect((x,y,x+3,y+3),shade)
        art.rect((11,11,16,16),'ink')
        art.rect((12,12,15,15),'leaf')
        frames.append(art.image)
    return frames
