"""Nine original pixel landscapes built from layered shapes and seeded details."""

import random
from PIL import Image
from pixels import PixelArt, star


def sky(art, colors):
    """Fill four broad sky bands without smooth gradients or sampled colours."""
    for y0,y1,shade in zip((0,24,46,67),(23,45,66,134),colors):
        art.rect((0,y0,239,y1),shade)


def cloud(art, x, y, width=30, shade='white'):
    """Draw a long stepped cloud with a shaded underside."""
    art.rect((x+6,y,x+width-10,y+3),shade)
    art.rect((x+2,y+3,x+width-3,y+6),shade)
    art.rect((x,y+6,x+width,y+8),shade)


def sun(art, x, y, shade='cream'):
    """Draw a stepped sun disc with a twelve-pixel diameter."""
    art.poly([(x-4,y-6),(x+4,y-6),(x+4,y-4),(x+6,y-4),(x+6,y+4),
              (x+4,y+4),(x+4,y+6),(x-4,y+6),(x-4,y+4),(x-6,y+4),
              (x-6,y-4),(x-4,y-4)],shade)


def ridge(art, heights, shade, bottom=135):
    """Draw a stepped terrain silhouette from evenly spaced authored heights."""
    step = 240 // (len(heights)-1)
    points = [(0,bottom),(0,heights[0])]
    for i,y in enumerate(heights[1:],1):
        x = i*step
        points.extend(((x,points[-1][1]),(x,y)))
    points.extend(((240,heights[-1]),(240,bottom)))
    art.poly(points,shade)


def pine(art, x, y, height=25, shade='pine', light='moss'):
    """Draw a tiered evergreen whose coordinates refer to its ground contact."""
    half = height//3
    art.rect((x-1,y-height//2,x+1,y),'earth')
    for level in range(3):
        top = y-height+level*(height//5)
        width = half*(level+2)//4
        art.poly([(x,top),(x+width,top+height//3),(x-width,top+height//3)],shade)
        art.line([(x-1,top+2),(x-width+2,top+height//3-1)],light)


def oak(art, x, y, height=23, shade='moss', light='leaf'):
    """Draw a broad angular tree crown and a forked trunk."""
    top=y-height
    crown=max(10,height*3//5)
    r=height//3
    art.rect((x-1,top+crown-3,x+1,y),'earth')
    art.line([(x,y-5),(x+r//2,top+crown-1)],'wood',2)
    art.poly([(x-r,top+crown//4),(x-r//3,top+crown//4),(x-r//3,top),
              (x+r//2,top),(x+r//2,top+crown//4),(x+r+2,top+crown//4),
              (x+r+2,top+3*crown//4),(x+r//3,top+3*crown//4),
              (x+r//3,top+crown),(x-r,top+crown)],shade)
    art.rect((x-r+1,top+crown//4+1,x-r//3,top+crown//2),light)
    art.rect((x-r//3+1,top+1,x+r//2-1,top+crown//4),light)
    art.rect((x+r//3,top+crown//2,x+r,top+3*crown//4-2),light)


def house(art, x, y, width=25, roof='red', wall='cream'):
    """Draw an original cottage with an offset chimney and lit square windows."""
    h = width//2
    art.rect((x+2,y-h,x+width-2,y),wall)
    art.rect((x+width-7,y-h-10,x+width-4,y-h-2),'earth')
    art.poly([(x-1,y-h),(x+width//2,y-h-11),(x+width+1,y-h)],'ink')
    art.poly([(x,y-h-1),(x+width//2,y-h-10),(x+width,y-h-1)],roof)
    art.line([(x+2,y-h-2),(x+width//2,y-h-9)],'rose' if roof=='red' else 'clay')
    art.rect((x+width//2-2,y-8,x+width//2+2,y),'earth')
    for wx in (x+5,x+width-7):
        art.rect((wx,y-h+4,wx+3,y-h+7),'navy')
        art.rect((wx,y-h+4,wx+1,y-h+5),'gold')
    art.rect((x+2,y,x+width-2,y+1),'earth')


def rocks(art, seed, zone, count, shades):
    """Scatter deterministic clusters inside a rectangular ground region."""
    rng = random.Random(seed)
    x0,y0,x1,y1 = zone
    for _ in range(count):
        x,y = rng.randint(x0,x1),rng.randint(y0,y1)
        width = rng.choice((2,3,5))
        art.rect((x,y,x+width,y),rng.choice(shades))


def water_lines(art, seed, zone, count=40, shade='aqua'):
    """Place short horizontal reflections on an authored water surface."""
    rng = random.Random(seed)
    x0,y0,x1,y1 = zone
    for _ in range(count):
        x,y = rng.randint(x0,x1),rng.randint(y0,y1)
        art.rect((x,y,min(x1,x+rng.randint(2,11)),y),shade)


def flowers(art, seed, zone, count=18, shade='cream'):
    """Place sparse two-pixel wildflowers over foreground grass."""
    rng = random.Random(seed)
    x0,y0,x1,y1 = zone
    for _ in range(count):
        x,y = rng.randint(x0,x1),rng.randint(y0,y1)
        art.rect((x,y,x,y+2),'pine')
        art.rect((x-1,y,x,y),shade)


def river(art):
    """Compose a river curving between orchard terraces toward distant hills."""
    sky(art,('#78b3ce','#92c5d8','#b0d8df','#d8e2ce'))
    sun(art,176,25)
    cloud(art,29,17,41)
    cloud(art,187,43,35,'mist')
    ridge(art,[65,61,48,43,56,62,52,57,63,52,59,65,62],'#729ca4')
    ridge(art,[80,68,64,65,70,78,80,72,68,70,78,69,75],'#5c8d70')
    art.poly([(0,82),(72,75),(106,77),(117,81),(99,90),(77,92),(52,104),(61,118),
              (111,135),(160,135),(83,116),(75,107),(101,98),(129,87),(128,78),
              (151,73),(240,88),(240,135),(0,135)],'green')
    art.poly([(111,76),(122,77),(119,83),(92,93),(66,100),(65,109),(88,119),(139,135),
              (103,135),(56,116),(48,106),(59,96),(89,87),(112,81)],'blue')
    art.line([(112,82),(90,89),(59,98)],'sky')
    art.line([(74,112),(83,117),(122,131)],'aqua',2)
    art.poly([(0,100),(20,98),(20,104),(36,104),(36,117),(44,117),(44,135),(0,135)],'pine')
    art.poly([(162,94),(240,87),(240,135),(162,135),(177,120),(164,113)],'moss')
    for x,y,h in ((18,86,22),(43,83,25),(66,85,19),(150,84,21),(199,91,30),(221,99,26)):
        oak(art,x,y,h)
    house(art,135,84,21,'wood','mist')
    art.line([(88,95),(119,97)],'earth',5)
    art.line([(87,92),(119,94)],'clay',2)
    for x in range(89,120,6):
        art.line([(x,91),(x,98)],'wood')
    flowers(art,101,(181,106,230,131),30)
    rocks(art,102,(80,71,104,76),12,['leaf','moss'])
    oak(art,21,136,50,'pine','moss')


def lake(art):
    """Compose a high alpine lake below asymmetric snow-capped peaks."""
    sky(art,('#658ea9','#85adbd','#b1c9cc','#d7dfd6'))
    cloud(art,19,17,51)
    cloud(art,159,27,47,'mist')
    art.poly([(0,78),(31,34),(48,49),(88,13),(144,75),(184,39),(240,82)],'#667888')
    art.poly([(47,71),(88,13),(144,75)],'#8b9b9e')
    art.poly([(69,39),(88,13),(113,41),(95,35),(87,42),(80,32)],'white')
    art.poly([(14,57),(31,34),(48,49),(43,54),(30,47)],'mist')
    art.poly([(166,58),(184,39),(202,56),(182,51)],'mist')
    ridge(art,[79,76,73,81,79,84,81,73,72,77,69,67,72],'pine')
    art.rect((0,84,239,120),'blue')
    water_lines(art,202,(5,87,234,115),55,'sky')
    art.poly([(0,106),(47,99),(83,109),(78,116),(103,122),(150,135),(0,135)],'moss')
    art.poly([(160,135),(189,118),(207,115),(213,107),(240,102),(240,135)],'pine')
    for x,y,h in ((12,111,47),(32,109,34),(210,116,39),(231,112,54),(181,124,22)):
        pine(art,x,y,h)
    art.poly([(49,123),(57,115),(77,114),(88,126)],'slate')
    art.line([(51,122),(59,116),(76,115)],'steel')
    flowers(art,203,(30,122,131,134),13,'lilac')


def cavern(art):
    """Compose a subterranean pool with a shaft of light and luminous crystals."""
    art.rect((0,0,239,134),'#172b3a')
    art.poly([(73,0),(106,0),(141,98),(41,97)],'#25444d')
    art.poly([(80,0),(99,0),(113,92),(63,92)],'#315c60')
    ridge(art,[75,68,60,66,71,66,59,71,68,59,63,55,65],'#294250')
    art.rect((21,90,219,121),'#34666c')
    water_lines(art,301,(24,94,210,115),44,'#55928e')
    art.poly([(0,0),(76,0),(71,7),(63,7),(63,15),(47,15),(47,24),(31,24),
              (31,45),(20,45),(20,91),(0,98)],'shadow')
    art.poly([(107,0),(240,0),(240,93),(218,90),(218,50),(200,50),(200,29),
              (176,29),(176,17),(139,17),(139,7),(107,7)],'shadow')
    for x,h in ((14,62),(37,46),(60,25),(141,29),(169,42),(205,74),(229,101)):
        art.poly([(x-5,0),(x+6,0),(x+3,h-5),(x,h)],'slate')
        art.line([(x-4,1),(x,h-4)],'#526876')
    art.poly([(0,107),(41,111),(52,120),(123,135),(0,135)],'ink')
    art.poly([(120,135),(160,115),(197,115),(211,104),(240,101),(240,135)],'ink')
    for x,y,h in ((28,115,20),(38,116,13),(181,119,27),(197,116,18),(174,120,16)):
        art.poly([(x-4,y),(x-4,y-h+5),(x,y-h),(x+4,y-h+5),(x+4,y)],'purple')
        art.poly([(x,y),(x,y-h),(x+3,y-h+5),(x+3,y)],'lilac')
        art.line([(x-3,y-h+6),(x,y-h+1)],'mist')
    art.poly([(74,82),(86,81),(95,87),(66,88)],'slate')
    art.rect((82,77,83,83),'wood')
    art.rect((80,75,85,78),'gold')
    art.rect((81,74,83,76),'cream')
    rocks(art,303,(0,121,239,134),46,['shadow','slate'])


def village(art):
    """Compose a golden-hour hillside hamlet with gardens and a winding path."""
    sky(art,('#ad7d99','#d5a0a0','#ebc09e','#eddaa9'))
    sun(art,52,29,'cream')
    cloud(art,136,18,60,'#ebc6ac')
    ridge(art,[76,70,64,65,59,56,65,61,65,75,70,74,76],'#817e80')
    ridge(art,[89,86,79,82,78,84,77,75,77,83,86,89,85],'#6e8464')
    art.rect((0,93,239,134),'moss')
    art.poly([(104,89),(115,89),(100,103),(146,134),(111,134),(85,104)],'clay')
    house(art,34,95,28,'red')
    house(art,79,86,23,'wood','mist')
    house(art,136,100,35,'red')
    house(art,194,91,24,'wood','mist')
    for x,y,h in ((19,97,31),(67,99,24),(125,90,25),(184,101,35),(230,97,29)):
        oak(art,x,y,h,'green','gold')
    art.poly([(25,113),(68,107),(84,119),(39,128)],'earth')
    for y in (112,117,122):
        art.line([(32,y),(65,y-4),(74,y+2)],'gold',2)
    for x in range(151,224,8):
        art.rect((x,114,x+1,123),'clay')
    art.line([(150,116),(228,116)],'wood',2)
    art.rect((194,116,196,132),'earth')
    art.rect((191,114,199,122),'ink')
    art.rect((192,116,198,120),'gold')
    flowers(art,403,(147,124,234,134),24,'rose')
    rocks(art,404,(3,130,100,134),15,['green','leaf'])


def coast(art):
    """Compose a moonlit rocky coast around a single warm lighthouse."""
    sky(art,('#27314d','#344466','#465b79','#667f90'))
    art.poly([(177,15),(182,15),(182,17),(184,17),(184,23),(182,23),
              (182,26),(177,26),(177,24),(175,24),(175,22),(179,22),
              (181,20),(181,18),(177,18)],'mist')
    for x,y in ((12,13),(59,29),(102,9),(148,37),(223,18),(206,48),(31,44)):
        star(art,x,y,'steel',1)
    art.rect((0,78,239,134),'navy')
    water_lines(art,501,(2,83,235,132),100,'#547d9c')
    art.poly([(0,88),(24,77),(50,80),(64,98),(89,110),(60,118),(0,116)],'ink')
    art.poly([(0,86),(24,74),(49,77),(59,92),(39,96),(0,92)],'moss')
    art.poly([(132,135),(149,114),(178,105),(199,110),(209,123),(240,126),(240,135)],'ink')
    art.poly([(174,106),(183,80),(199,83),(207,116)],'shadow')
    art.rect((31,43,44,78),'mist')
    art.rect((31,60,44,66),'red')
    art.rect((29,35,46,43),'ink')
    art.rect((31,36,44,41),'gold')
    art.rect((34,37,41,40),'cream')
    art.poly([(27,34),(37,26),(48,34)],'red')
    art.rect((30,43,45,45),'steel')
    art.rect((36,70,39,78),'navy')
    art.line([(0,99),(25,102),(50,100),(65,106)],'sky')
    art.line([(151,128),(164,116),(178,115)],'sky')
    art.line([(139,83),(158,83)],'mist')
    art.line([(131,88),(167,88)],'steel')
    art.line([(142,93),(155,93)],'steel')


def oasis(art):
    """Compose a broad desert arch enclosing a blue oasis and date palms."""
    sky(art,('#7fb7c5','#a0cdd2','#c8ded5','#ece0b4'))
    sun(art,196,24)
    ridge(art,[82,77,77,67,67,72,72,64,64,77,82,80,83],'#c09a6e')
    art.rect((0,92,239,134),'gold')
    art.poly([(36,0),(57,0),(57,6),(135,6),(135,15),(160,15),(160,41),
              (154,41),(154,101),(127,101),(127,50),(119,50),(119,35),
              (106,30),(69,30),(62,36),(57,36),(57,99),(20,108),(20,39),(27,39),(27,10),(36,10)],'ochre')
    art.poly([(36,0),(57,0),(57,6),(135,6),(135,15),(153,15),(153,29),
              (132,29),(124,20),(62,20),(46,33),(46,88),(27,96),(27,33),(36,33)],'clay')
    art.line([(38,9),(52,9),(52,15),(130,15)],'cream',2)
    art.line([(24,70),(43,65)],'gold',2)
    art.line([(132,76),(151,76)],'clay',3)
    art.poly([(78,102),(124,93),(166,101),(186,115),(164,128),(106,131),(65,119)],'teal')
    art.poly([(82,105),(125,98),(162,105),(176,114),(158,121),(105,124),(75,117)],'blue')
    water_lines(art,601,(99,107,152,120),20,'sky')
    for x,y,h in ((190,108,43),(213,113,30),(75,103,26)):
        art.line([(x,y),(x-3,y-h//2),(x-1,y-h)],'wood',3)
        for dx,dy in ((-17,0),(-12,-6),(11,-7),(18,1),(12,8)):
            art.line([(x-1,y-h),(x+dx//2,y-h+dy-3),(x+dx,y-h+dy)],'pine',3)
    rocks(art,604,(6,127,230,134),28,['ochre','cream','clay'])


def snow(art):
    """Compose a snowy pine valley with a warm cabin and a frozen stream."""
    sky(art,('#99b0c7','#bacbd9','#d4dedf','#e8e8da'))
    cloud(art,46,19,50,'white')
    art.poly([(0,83),(46,32),(77,57),(127,18),(189,70),(211,43),(240,82)],'slate')
    art.poly([(88,59),(127,18),(163,57),(141,51),(126,43),(113,56),(107,48)],'white')
    art.poly([(25,56),(46,32),(66,54),(49,47),(39,53)],'mist')
    ridge(art,[91,83,86,79,85,90,93,85,83,86,79,83,88],'#76999c')
    art.rect((0,101,239,134),'mist')
    art.poly([(112,98),(124,98),(110,109),(155,135),(109,135),(95,110)],'sky')
    art.line([(117,102),(104,110),(117,119)],'white',2)
    for x,y,h in ((9,103,48),(33,105,40),(62,99,33),(175,105,34),(198,106,47),(229,110,55)):
        pine(art,x,y,h,'pine','mist')
    house(art,136,105,27,'wood','clay')
    art.poly([(133,91),(149,79),(166,91),(155,88),(149,84),(141,91)],'white')
    art.rect((158,72,160,78),'steel')
    art.rect((155,64,159,71),'mist')
    art.poly([(0,119),(45,115),(80,126),(93,135),(0,135)],'white')
    art.poly([(175,125),(220,116),(240,119),(240,135),(154,135)],'white')
    rocks(art,702,(164,126,218,134),12,['steel','mist'])
    for x,y in ((145,111),(141,115),(145,119),(149,123)):
        art.rect((x,y,x+1,y),'slate')


def canyon(art):
    """Compose warm canyon walls around a narrow river and suspended bridge."""
    sky(art,('#678ba8','#97b5c2','#c5d1ca','#e1d5b1'))
    cloud(art,86,20,55)
    ridge(art,[79,69,69,81,87,72,68,78,67,66,66,79,77],'#9f8990')
    art.rect((0,88,239,134),'earth')
    art.poly([(112,89),(126,89),(117,107),(144,135),(100,135),(103,108)],'teal')
    art.line([(120,98),(112,110),(126,127)],'aqua')
    art.poly([(0,26),(44,26),(44,37),(68,37),(68,52),(83,52),(83,79),
              (70,79),(70,114),(53,114),(53,135),(0,135)],'wood')
    art.poly([(0,26),(44,26),(44,37),(68,37),(68,52),(81,52),(81,64),
              (58,64),(58,53),(34,53),(34,43),(0,43)],'clay')
    art.poly([(167,45),(191,45),(191,26),(240,26),(240,135),(196,135),
              (196,111),(180,111),(180,84),(163,84),(163,59),(167,59)],'clay')
    for y,x1,x2 in ((67,0,68),(82,0,63),(98,0,60),(117,0,44),(52,188,239),(72,175,239),(95,185,239),(118,204,239)):
        art.rect((x1,y,x2,y+3),'ochre')
        art.rect((x1,y+4,x2-5,y+5),'gold')
    art.line([(66,70),(97,83),(131,88),(171,69)],'earth',2)
    art.line([(66,82),(96,95),(130,98),(173,80)],'wood',5)
    for x,y in ((70,72),(80,76),(90,80),(101,83),(112,85),(124,86),(135,83),(147,78),(159,74),(170,70)):
        art.line([(x,y),(x,y+13)],'gold')
    art.rect((62,62,65,85),'earth')
    art.rect((173,61,176,83),'earth')
    art.rect((22,14,24,26),'pine')
    art.line([(17,17),(17,21),(23,21),(30,21),(30,12)],'pine',3)
    rocks(art,802,(4,126,52,134),20,['earth','ochre','gold'])


def ruins(art):
    """Compose an overgrown island shrine with lily pads in a quiet marsh."""
    sky(art,('#718e9e','#96b4b7','#b7cdbe','#d8dec0'))
    cloud(art,142,21,63,'mist')
    ridge(art,[70,65,62,60,67,70,67,62,64,60,66,63,69],'#7f9d8d')
    ridge(art,[89,84,78,81,89,85,83,79,80,88,79,82,87],'#557d69')
    art.rect((0,92,239,134),'teal')
    water_lines(art,901,(0,96,239,134),80,'#7bad9c')
    art.poly([(68,108),(75,96),(115,85),(160,92),(175,106),(138,119),(87,117)],'earth')
    art.poly([(68,104),(79,94),(116,83),(160,90),(175,102),(136,112),(88,110)],'moss')
    art.rect((104,62,111,96),'slate')
    art.rect((136,55,145,96),'slate')
    art.rect((100,53,150,61),'steel')
    art.rect((106,46,130,53),'slate')
    art.rect((134,47,145,53),'steel')
    art.rect((102,54,148,56),'mist')
    art.rect((106,63,108,89),'mist')
    art.rect((137,63,140,88),'steel')
    art.rect((101,94,115,98),'steel')
    art.rect((133,94,148,98),'steel')
    art.rect((118,95,127,102),'slate')
    art.rect((114,103,132,106),'steel')
    art.line([(143,50),(144,69),(137,74),(137,86)],'pine',2)
    for x,y in ((139,58),(142,65),(134,77),(136,85)):
        art.rect((x,y,x+4,y+2),'leaf')
    oak(art,85,99,40,'pine','green')
    oak(art,165,104,31,'moss','leaf')
    oak(art,17,130,70,'pine','moss')
    for x,y in ((45,120),(66,128),(178,119),(199,126),(214,111)):
        art.rect((x,y,x+8,y+2),'pine')
        art.rect((x+4,y-1,x+8,y),'leaf')
    flowers(art,905,(82,101,102,109),7,'rose')
    star(art,124,75,'cream',1)


SCENES = {
    'river':river, 'lake':lake, 'cavern':cavern, 'village':village,
    'coast':coast, 'oasis':oasis, 'snow':snow, 'canyon':canyon, 'ruins':ruins,
}


def scene(name, size):
    """Render a named composition at the required output size using nearest pixels."""
    art = PixelArt(240,135)
    SCENES[name](art)
    # The preview uses its own 160×90 grid so every output pixel block is 5×5.
    logical = art.image
    if size == (800,450):
        logical = logical.resize((160,90),Image.Resampling.NEAREST)
    return logical.resize(size,Image.Resampling.NEAREST)
