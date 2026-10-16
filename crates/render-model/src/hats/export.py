"""Export the original Blockbench project into the runtime's Bedrock mesh format."""
import argparse
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def normal(points):
    """Return an authored polygon's outward normal in Bedrock's winding convention."""
    a, b, c = points[:3]
    u, v = ([b[i] - a[i] for i in range(3)], [c[i] - a[i] for i in range(3)])
    cross = [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]]
    length = math.sqrt(sum(n*n for n in cross)) or 1
    return [-n / length for n in cross]


def export(project):
    """Derive bone ancestry, connected pixel surfaces and flat polygon normals."""
    elements = {element['uuid']: element for element in project['elements']}
    groups = {group['uuid']: group for group in project['groups']}
    bones = []

    def visit(node, parent=None):
        group = groups[node['uuid']]
        if any(group.get('rotation', [0, 0, 0])):
            raise ValueError('Bake group rotations into the headwear surface before exporting')
        bone = {'name': group['name'], 'pivot': group['origin']}
        if parent:
            bone['parent'] = parent
        bones.append(bone)
        for child in node['children']:
            if isinstance(child, dict):
                visit(child, group['name'])
                continue
            element = elements[child]
            if element['type'] == 'cube':
                cube = {'origin': element['from'], 'size': [element['to'][i]-element['from'][i] for i in range(3)], 'uv': {}}
                for face, value in element['faces'].items():
                    uv = value['uv']
                    cube['uv'][face] = {'uv': uv[:2], 'uv_size': [uv[2]-uv[0], uv[3]-uv[1]]}
                bone.setdefault('cubes', []).append(cube)
                continue
            if any(element.get('origin', [0, 0, 0])) or any(element.get('rotation', [0, 0, 0])):
                raise ValueError('Bake mesh transforms into its vertices before exporting')
            keys = list(element['vertices'])
            positions = [element['vertices'][key] for key in keys]
            index = {key: i for i, key in enumerate(keys)}
            normals, uvs, polys = [], [], []
            for face in element['faces'].values():
                direction = normal([element['vertices'][key] for key in face['vertices']])
                normal_index = len(normals)
                normals.append(direction)
                polygon = []
                for key in face['vertices']:
                    uv = face['uv'][key]
                    if uv not in uvs:
                        uvs.append(uv)
                    polygon.append([index[key], normal_index, uvs.index(uv)])
                polys.append(polygon)
            bone['poly_mesh'] = {'normalized_uvs': False, 'positions': positions, 'normals': normals, 'uvs': uvs, 'polys': polys}

    for node in project['outliner']:
        visit(node)
    return {'format_version': '1.12.0', 'minecraft:geometry': [{'description': {
        'identifier': project['model_identifier'], 'texture_width': project['resolution']['width'], 'texture_height': project['resolution']['height'],
        'visible_bounds_width': 2, 'visible_bounds_height': 3.5, 'visible_bounds_offset': [0, 1.5, 0]
    }, 'bones': bones}]}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('project', nargs='?', default='bunny_ears', help='Blockbench project name or path')
    name = Path(parser.parse_args().project)
    if not name.suffix:
        name = name.with_suffix('.bbmodel')
    source = name if name.is_absolute() else ROOT / name
    project = json.loads(source.read_text())
    source.with_suffix('.geo.json').write_text(json.dumps(export(project), separators=(',', ':')) + '\n')
