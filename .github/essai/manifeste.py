"""Montre le manifeste (ressource de type 24) incorporé à un exécutable Windows."""
import struct
import sys
import xml.parsers.expat


def manifests(path):
    data = open(path, "rb").read()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    sections = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    directories = optional + (112 if magic == 0x20B else 96)
    resource_rva, _ = struct.unpack_from("<II", data, directories + 2 * 8)
    table = optional + optional_size
    layout = []
    for index in range(sections):
        entry = table + index * 40
        virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from("<IIII", data, entry + 8)
        layout.append((virtual_address, max(virtual_size, raw_size), raw_offset))

    def offset_of(rva):
        for virtual_address, size, raw_offset in layout:
            if virtual_address <= rva < virtual_address + size:
                return raw_offset + rva - virtual_address
        raise ValueError(hex(rva))

    root = offset_of(resource_rva)

    def entries(directory):
        named, ids = struct.unpack_from("<HH", data, directory + 12)
        for index in range(named + ids):
            yield struct.unpack_from("<II", data, directory + 16 + index * 8)

    for kind, target in entries(root):
        if kind != 24:
            continue
        for name, target in entries(root + (target & 0x7FFFFFFF)):
            for language, target in entries(root + (target & 0x7FFFFFFF)):
                rva, size = struct.unpack_from("<II", data, root + target)
                start = offset_of(rva)
                yield name, language, data[start:start + size]


for name, language, content in manifests(sys.argv[1]):
    print(f"manifeste n° {name}, langue {language}, {len(content)} octets")
    print("premiers octets :", content[:64].hex(" "))
    print(ascii(content.decode("utf-8", errors="backslashreplace")))
    parser = xml.parsers.expat.ParserCreate()
    try:
        parser.Parse(content, True)
        print("XML : bien forme")
    except xml.parsers.expat.ExpatError as error:
        print("XML : MAL FORME,", error)
