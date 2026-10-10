"""Original quiet desk vignettes, error illustrations, and pack covers."""

from icons import chest, envelope, icon
from pixels import PixelArt, cube, enlarge, star


def ground(art):
    """Place a broken ground line and sparse grass below a vignette."""
    art.rect((27,41,102,42),'shadow')
    art.rect((17,42,22,42),'slate')
    art.rect((107,42,112,42),'slate')
    for x,y in ((32,37),(95,38),(23,39)):
        art.line([(x,y+3),(x-1,y),(x+1,y+3),(x+3,y-1)],'pine')


def cloud(art, x, y):
    """Draw a small low-contrast stepped cloud behind the main object."""
    art.rect((x+4,y,x+16,y+2),'shadow')
    art.rect((x,y+3,x+23,y+4),'shadow')


def mailbox(art):
    """Draw an empty blue post box with a lowered flag and a waiting bird."""
    art.rect((58,25,62,40),'wood')
    art.rect((56,39,65,40),'earth')
    art.poly([(43,25),(43,13),(47,9),(69,9),(74,14),(74,25)],'ink')
    art.poly([(44,24),(44,14),(48,10),(68,10),(72,14),(72,24)],'blue')
    art.rect((48,14,65,23),'navy')
    art.rect((50,16,63,22),'ink')
    art.rect((47,25,66,27),'steel')
    art.rect((70,17,79,18),'red')
    art.rect((69,16,71,21),'rose')
    art.rect((79,34,87,38),'gold')
    art.rect((83,31,87,36),'gold')
    art.rect((87,33,89,34),'clay')
    art.rect((86,32,86,32),'ink')
    art.line([(81,39),(81,41)],'ochre')
    art.line([(85,39),(85,41)],'ochre')


def tower(art):
    """Draw a quiet signal tower on its own floating garden island."""
    art.poly([(38,31),(66,25),(91,31),(66,45)],'plum')
    art.poly([(38,31),(66,25),(91,31),(66,36)],'moss')
    art.rect((57,12,72,30),'ink')
    art.rect((59,13,70,30),'steel')
    art.rect((58,7,71,13),'shadow')
    art.rect((60,8,69,12),'gold')
    art.rect((62,9,67,11),'cream')
    art.poly([(55,7),(64,1),(74,7)],'purple')
    art.rect((63,22,66,30),'navy')
    art.rect((45,28,51,30),'leaf')
    art.rect((78,29,82,31),'leaf')
    star(art,86,12,'lilac',2)
    star(art,39,15,'lilac',1)


def waiting_letter(art):
    """Draw an unopened invitation resting on a small outdoor bench."""
    art.rect((40,29,86,32),'wood')
    art.rect((42,23,83,25),'clay')
    for x in (44,79):
        art.rect((x,24,x+2,40),'earth')
    envelope(art,55,14)
    art.rect((88,30,98,39),'clay')
    art.rect((87,29,99,31),'cream')
    art.line([(93,29),(93,18)],'moss')
    art.poly([(93,24),(87,21),(88,18),(93,21)],'leaf')
    art.poly([(93,21),(99,15),(101,18),(94,24)],'green')


def satchel(art):
    """Draw a ready-to-travel satchel with a blank ticket tucked in its pocket."""
    art.rect((54,10,73,13),'ink')
    art.rect((56,9,71,11),'clay')
    art.rect((54,12,57,17),'clay')
    art.rect((70,12,73,17),'wood')
    art.poly([(49,16),(79,16),(83,21),(83,39),(46,39),(46,22)],'ink')
    art.rect((48,20,81,37),'ochre')
    art.rect((48,18,79,26),'gold')
    art.rect((61,24,67,30),'ink')
    art.rect((63,25,65,28),'cream')
    art.rect((50,30,57,36),'clay')
    art.rect((72,29,85,35),'white')
    art.rect((75,31,81,32),'purple')
    star(art,39,16,'gold',2)


def message_desk(art):
    """Draw two blank feedback cards above a desk with a pencil cup."""
    art.rect((38,33,88,36),'wood')
    art.rect((43,37,46,40),'earth')
    art.rect((81,37,84,40),'earth')
    art.poly([(42,7),(69,7),(69,22),(57,22),(52,27),(52,22),(42,22)],'ink')
    art.rect((44,9,67,20),'mist')
    art.rect((48,13,61,14),'teal')
    art.rect((48,17,56,17),'slate')
    art.poly([(65,15),(87,15),(87,27),(81,27),(81,30),(77,27),(65,27)],'purple')
    art.rect((70,19,81,20),'lilac')
    art.rect((71,30,80,32),'white')
    art.rect((39,26,47,32),'blue')
    art.line([(42,28),(40,20)],'gold',2)
    art.line([(45,28),(47,21)],'rose',2)


def telescope(art):
    """Draw a telescope aimed at an empty patch of sky."""
    art.line([(60,27),(49,40)],'wood',2)
    art.line([(60,27),(70,40)],'wood',2)
    art.line([(60,27),(61,41)],'earth',2)
    art.line([(43,23),(76,9)],'ink',10)
    art.line([(44,22),(74,9)],'blue',7)
    art.line([(43,20),(74,7)],'sky',2)
    art.line([(72,5),(77,14)],'steel',3)
    art.rect((40,23,45,26),'slate')
    star(art,93,7,'cream',2)
    star(art,84,20,'steel',1)
    star(art,29,11,'steel',1)


def fallen_sign(art):
    """Draw a crooked warning sign and a few loose stones."""
    art.line([(63,24),(68,40)],'wood',4)
    art.poly([(61,3),(80,30),(40,30)],'ink')
    art.poly([(61,7),(75,27),(45,27)],'gold')
    art.rect((59,14,62,20),'earth')
    art.rect((59,23,62,24),'earth')
    for x,y,w in ((39,37,10),(77,35,9),(86,39,7)):
        art.poly([(x,y+3),(x+2,y),(x+w-2,y),(x+w,y+3)],'slate')
    art.rect((48,33,53,35),'steel')


def unplugged(art):
    """Draw two separated cable plugs with a small gap and visible contacts."""
    art.line([(27,37),(39,37),(39,29),(49,29)],'slate',3)
    art.line([(79,23),(89,23),(89,37),(103,37)],'slate',3)
    art.rect((45,21,56,33),'ink')
    art.rect((46,22,55,32),'blue')
    art.rect((56,24,60,25),'mist')
    art.rect((56,29,60,30),'mist')
    art.rect((71,17,83,29),'ink')
    art.rect((72,18,82,28),'teal')
    art.rect((73,21,75,22),'ink')
    art.rect((73,25,75,26),'ink')
    art.line([(62,12),(64,17)],'gold')
    art.line([(69,9),(68,15)],'gold')
    art.line([(63,35),(65,32)],'gold')


def vignette(name):
    """Render a transparent 128×48 composition; profile errors use exact 2× pixels."""
    art = PixelArt(128,48)
    cloud(art,17,13)
    cloud(art,90,22)
    ground(art)
    painters = {
        'news':mailbox, 'realms':tower, 'invites':waiting_letter,
        'pass':satchel, 'feedback':message_desk, 'nothing':telescope,
        'generic':fallen_sign, 'connection':unplugged,
    }
    painters[name](art)
    return enlarge(art.image,2) if name in ('nothing','generic','connection') else art.image


def overworld_block():
    """Draw an original grassy soil block, with stones and blades rather than game textures."""
    art = PixelArt(28,28)
    cube(art,2,2,24,'leaf','clay','earth')
    art.poly([(3,9),(13,13),(13,16),(10,15),(10,14),(6,13),(6,12),(3,11)],'green')
    art.poly([(14,14),(25,9),(25,12),(22,13),(22,15),(18,16),(18,17),(14,18)],'moss')
    art.line([(9,6),(11,7),(14,6)],'cream')
    art.line([(17,7),(20,7)],'green')
    art.rect((6,16,8,17),'wood')
    art.rect((10,21,11,22),'cream')
    art.rect((18,20,20,21),'wood')
    art.rect((22,15,23,16),'clay')
    return enlarge(art.image,4)


def pack_cover(missing=False):
    """Draw an original resource-pack case or missing-pack symbol at its exact size."""
    if missing:
        art = PixelArt(32,32)
        art.poly([(6,7),(12,3),(26,3),(26,26),(20,29),(6,26)],'ink')
        art.rect((8,8,22,25),'purple')
        art.poly([(8,6),(13,4),(24,4),(21,6)],'lilac')
        art.poly([(23,8),(25,6),(25,25),(23,26)],'plum')
        art.grid(['.####.','##..##','....##','...##.','..##..','......','..##..'],{'#':'white'},(12,12))
        return enlarge(art.image,2)
    art = PixelArt(64,64,'navy')
    art.rect((2,2,61,61),'ink')
    art.rect((4,4,59,59),'sky')
    art.rect((4,33,59,59),'teal')
    art.rect((46,9,52,15),'cream')
    art.poly([(4,32),(15,19),(29,35),(40,22),(59,38),(59,51),(4,51)],'moss')
    art.poly([(4,48),(25,37),(45,43),(59,38),(59,59),(4,59)],'pine')
    art.poly([(23,43),(35,40),(49,59),(18,59)],'blue')
    art.rect((7,8,32,10),'white')
    art.rect((7,12,22,13),'white')
    cube(art,19,19,26,'leaf','clay','earth')
    art.line([(26,24),(30,26),(36,23)],'cream')
    art.rect((5,55,14,58),'gold')
    art.rect((7,54,8,58),'cream')
    return enlarge(art.image,4)
