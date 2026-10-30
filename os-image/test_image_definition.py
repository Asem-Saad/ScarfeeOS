"""Checks for the image definition. Standard library only.

Run: python3 os-image/test_image_definition.py

This checks that config.xml is well-formed XML and follows the project's
policy. If kiwi-ng is on PATH it also validates config.xml against kiwi's own
schema (that test is skipped otherwise). It does not build anything.
"""
import re
import shutil
import subprocess
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).resolve().parent

PATENT_CODEC_PATTERN = re.compile(r"(openh264|x264|x265|ffmpeg|libav|fdk-aac|faac|hevc|h26[45])")
REQUIRED_FONTS = [
    "google-noto-naskh-arabic-fonts",
    "google-noto-kufi-arabic-fonts",
    "ibm-plex-sans-arabic-fonts",
]


def read_package_list():
    sections, current = {}, None
    for raw in (HERE / "packages.txt").read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            current = line[1:-1]
            sections[current] = []
        else:
            sections[current].append(line)
    return sections


def read_config_packages(root):
    sections = {"bootstrap": [], "image": [], "ignore": []}
    for block in root.findall("packages"):
        kind = block.get("type")
        sections[kind].extend(p.get("name") for p in block.findall("package"))
        sections["ignore"].extend(p.get("name") for p in block.findall("ignore"))
    return sections


class ImageDefinition(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = ET.parse(HERE / "config.xml").getroot()
        cls.config = read_config_packages(cls.root)
        cls.listed = read_package_list()

    def test_xml_is_well_formed(self):
        self.assertEqual(self.root.tag, "image")
        self.assertEqual(self.root.get("name"), "ScarfeeOS")
        for tag in ("description", "preferences", "repository", "packages"):
            self.assertIsNotNone(self.root.find(tag), tag)

    def test_package_list_matches_config(self):
        for section in ("bootstrap", "image", "ignore"):
            self.assertEqual(sorted(self.config[section]), sorted(self.listed[section]), section)

    def test_no_duplicate_packages(self):
        for section in ("bootstrap", "image"):
            names = self.listed[section]
            self.assertEqual(len(names), len(set(names)), section)

    def test_arabic_fonts_and_input_present(self):
        installed = set(self.config["image"])
        for font in REQUIRED_FONTS:
            self.assertIn(font, installed)
        self.assertIn("ibus", installed)
        self.assertTrue({"ibus-m17n", "ibus-typing-booster"} & installed)

    def test_no_patent_codecs_installed(self):
        for name in self.config["bootstrap"] + self.config["image"]:
            self.assertIsNone(PATENT_CODEC_PATTERN.search(name), name)

    def test_codecs_are_on_ignore_list(self):
        ignored = set(self.config["ignore"])
        for name in ("openh264", "x264", "x265", "ffmpeg"):
            self.assertIn(name, ignored)

    def test_no_fedora_branded_packages_installed(self):
        for name in self.config["bootstrap"] + self.config["image"]:
            self.assertFalse(name.startswith("fedora-"), name)
        self.assertIn("generic-release", self.config["image"])

    def test_no_logo_package_installed(self):
        # generic-logos ships a KDE logo and files named fedora-*; no logo package until ScarfeeOS has its own.
        for name in self.config["bootstrap"] + self.config["image"]:
            self.assertNotIn("logos", name)
        self.assertIn("generic-logos", self.config["ignore"])

    def test_contact_is_set(self):
        contact = self.root.find("description/contact").text
        self.assertEqual(contact, "Asem Y Saad <asem.y.saad@outlook.com>")

    def test_repositories_use_one_supported_release(self):
        releases = set()
        for repo in self.root.findall("repository"):
            url = repo.find("source").get("path")
            self.assertTrue(url.startswith("https://"), url)
            match = re.search(r"/(?:releases|updates)/(\d+)/Everything/x86_64/", url)
            self.assertIsNotNone(match, url)
            releases.add(match.group(1))
        self.assertEqual(len(releases), 1, releases)

    @unittest.skipUnless(shutil.which("kiwi-ng"), "kiwi-ng is not installed")
    def test_kiwi_schema_validation(self):
        result = subprocess.run(
            ["kiwi-ng", "image", "info", "--description", str(HERE)],
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_no_fedora_trademark_in_image_identity(self):
        self.assertNotIn("fedora", self.root.get("name").lower())
        spec = self.root.find("description/specification").text.lower()
        self.assertNotIn("fedora", spec)
        self.assertEqual(self.root.find("preferences/type").get("volid"), "SCARFEEOS")


if __name__ == "__main__":
    unittest.main()
