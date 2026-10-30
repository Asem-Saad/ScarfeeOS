"""Checks for the image definition. Standard library only.

Run: python3 os-image/test_image_definition.py

This checks that config.xml is well-formed XML and follows the project's
policy. If kiwi-ng is on PATH it also validates config.xml against kiwi's own
schema (that test is skipped otherwise). It does not build anything.
"""
import re
import shutil
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).resolve().parent

PATENT_CODEC_PATTERN = re.compile(r"(openh264|x264|x265|ffmpeg|libav|fdk-aac|faac|hevc|h26[45])")
PROJECT_URL = "https://github.com/Asem-Saad/ScarfeeOS"
RELEASE_DIR = HERE / "scarfeeos-release"
LOGOS_DIR = HERE / "scarfeeos-logos"
REPOS_DIR = HERE / "scarfeeos-repos"
# A repository whose id, name or address matches this serves patent-licensed codecs (docs/ip-and-licensing.md rule 4).
CODEC_REPO_PATTERN = re.compile(r"(cisco|openh264|h\.?26[45]|x26[45]|hevc|ffmpeg|freeworld|rpmfusion)", re.IGNORECASE)
REPO_ADDRESS_KEYS = ("baseurl", "metalink", "mirrorlist")
GPG_KEY_PATH = "file:///etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch"
# Names that must not appear in user-visible release or logo fields.
FORBIDDEN_NAMES = re.compile(
    r"(fedora|red ?hat|rhel|centos|generic|ubuntu|debian|suse|windows|microsoft|"
    r"apple|macos|google|chrome|android|gnome|kde|vlc)", re.IGNORECASE)
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


def read_spec(path):
    """Return the spec's header tags (first occurrence of each) and its full text."""
    text = path.read_text(encoding="utf-8")
    tags = {}
    for match in re.finditer(r"^([A-Za-z]+\d*):\s*(.+?)\s*$", text, re.MULTILINE):
        tags.setdefault(match.group(1), match.group(2))
    return tags, text


def read_os_release(path):
    fields = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            key, _, value = line.partition("=")
            fields[key] = value.strip('"')
    return fields


def read_config_packages(root):
    sections = {"bootstrap": [], "image": [], "ignore": []}
    for block in root.findall("packages"):
        kind = block.get("type")
        sections[kind].extend(p.get("name") for p in block.findall("package"))
        sections["ignore"].extend(p.get("name") for p in block.findall("ignore"))
    return sections


def read_repo_file(text):
    """Return {repo id: {key: value}} for a dnf .repo file."""
    repos, current = {}, None
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith(("#", ";")):
            continue
        if line.startswith("[") and line.endswith("]"):
            current = repos.setdefault(line[1:-1], {})
        elif current is not None and "=" in line:
            key, _, value = line.partition("=")
            current[key.strip()] = value.strip()
    return repos


def enabled_codec_repos(repos):
    """Ids of enabled repositories that look like a Cisco, H.264 or other codec repository.
    dnf treats a repository with no `enabled` key as enabled."""
    found = []
    for repo_id, options in repos.items():
        if options.get("enabled", "1") != "1":
            continue
        fields = [repo_id, options.get("name", "")] + [options.get(k, "") for k in REPO_ADDRESS_KEYS]
        if any(CODEC_REPO_PATTERN.search(f) for f in fields):
            found.append(repo_id)
    return found


# fedora-repos 44-2 ships this as fedora-cisco-openh264.repo, enabled by default (shortened).
FEDORA_CISCO_REPO = """
[fedora-cisco-openh264]
name=Fedora $releasever openh264 (From Cisco) - $basearch
metalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-cisco-openh264-$releasever&arch=$basearch
type=rpm
enabled=1
gpgcheck=1

[fedora-cisco-openh264-debuginfo]
name=Fedora $releasever openh264 (From Cisco) - $basearch - Debug
metalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-cisco-openh264-debug-$releasever&arch=$basearch
enabled=0
"""


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
        self.assertIn("scarfeeos-release", self.config["image"])
        self.assertNotIn("generic-release", self.config["image"])
        for name in ("generic-release", "generic-release-common"):
            self.assertIn(name, self.config["ignore"])

    def test_only_scarfeeos_logo_package_installed(self):
        # generic-logos ships a KDE logo and files named fedora-*; scarfeeos-logos is the only logo package.
        installed = self.config["bootstrap"] + self.config["image"]
        self.assertIn("scarfeeos-logos", installed)
        for name in installed:
            if "logos" in name:
                self.assertEqual(name, "scarfeeos-logos")
        self.assertIn("generic-logos", self.config["ignore"])

    def test_local_repository_holds_scarfeeos_packages(self):
        repos = {r.get("alias"): r for r in self.root.findall("repository")}
        self.assertEqual(repos["scarfeeos"].get("type"), "rpm-md")
        self.assertEqual(repos["scarfeeos"].find("source").get("path"), "this://repo")
        # lower number wins, so the ScarfeeOS packages beat any other provider
        self.assertLess(int(repos["scarfeeos"].get("priority")), int(repos["base"].get("priority")))

    def test_contact_is_set(self):
        contact = self.root.find("description/contact").text
        self.assertEqual(contact, "Asem Y Saad <asem.y.saad@outlook.com>")

    def test_repositories_use_one_supported_release(self):
        releases = set()
        for repo in self.root.findall("repository"):
            url = repo.find("source").get("path")
            if url.startswith("this://"):
                continue
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

    def test_release_spec_fields(self):
        tags, text = read_spec(RELEASE_DIR / "scarfeeos-release.spec")
        self.assertEqual(tags["Name"], "scarfeeos-release")
        self.assertEqual(tags["License"], "Apache-2.0")
        self.assertEqual(tags["URL"], PROJECT_URL)
        self.assertIn("Provides:       system-release = %{version}-%{release}", text)
        self.assertIn("Provides:       system-release(%{version})", text)
        # Repository definitions come from scarfeeos-repos; fedora-repos is not required (README.md).
        self.assertEqual(tags["Requires"], "scarfeeos-repos(%{version})")
        self.assertNotIn("fedora-repos", text)
        for conflict in ("fedora-release", "generic-release"):
            self.assertIn("Conflicts:      " + conflict + "\n", text)
        for name in re.findall(r"^Source\d+:\s*(\S+)", text, re.MULTILINE):
            self.assertTrue((RELEASE_DIR / name).is_file(), name)

    def test_release_files_name_scarfeeos_only(self):
        tags, text = read_spec(RELEASE_DIR / "scarfeeos-release.spec")
        osr = read_os_release(RELEASE_DIR / "os-release")
        self.assertEqual(osr["NAME"], "ScarfeeOS")
        self.assertEqual(osr["ID"], "scarfeeos")
        self.assertNotIn("ID_LIKE", osr)
        self.assertEqual(osr["HOME_URL"], PROJECT_URL)
        urls = [v for k, v in osr.items() if k.endswith("_URL")]
        self.assertGreaterEqual(len(urls), 3)
        for url in urls:
            self.assertTrue(url.startswith(PROJECT_URL), url)
        visible = [tags["Summary"], text.split("%description")[1].split("%prep")[0]]
        for name in ("os-release", "scarfeeos-release", "issue", "system-release-cpe"):
            visible.append((RELEASE_DIR / name).read_text(encoding="utf-8"))
        for value in visible:
            self.assertIsNone(FORBIDDEN_NAMES.search(value), value)

    def test_release_version_is_consistent(self):
        version = self.root.find("preferences/version").text
        osr = read_os_release(RELEASE_DIR / "os-release")
        self.assertEqual(osr["VERSION_ID"], version)
        self.assertEqual(osr["VERSION"], version)
        self.assertEqual(osr["PRETTY_NAME"], "ScarfeeOS " + version)
        self.assertEqual(osr["CPE_NAME"], "cpe:/o:scarfeeos:scarfeeos:" + version)
        self.assertEqual((RELEASE_DIR / "scarfeeos-release").read_text().strip(), "ScarfeeOS release " + version)
        self.assertEqual((RELEASE_DIR / "system-release-cpe").read_text().strip(), osr["CPE_NAME"])

    def test_release_preset_ends_with_disable_all(self):
        lines = [l for l in (RELEASE_DIR / "90-scarfeeos.preset").read_text().splitlines()
                 if l and not l.startswith("#")]
        self.assertEqual(lines[-1], "disable *")
        self.assertTrue(all(l.startswith("enable ") for l in lines[:-1]))

    def test_license_files_are_apache(self):
        for directory in (RELEASE_DIR, LOGOS_DIR, REPOS_DIR):
            text = (directory / "LICENSE").read_text(encoding="utf-8")
            self.assertIn("Apache License", text)
            self.assertIn("Version 2.0", text)
            self.assertEqual(text, (HERE.parent / "LICENSE").read_text(encoding="utf-8"))

    def test_logo_is_plain_and_original(self):
        svg = (LOGOS_DIR / "scarfeeos-logo.svg").read_text(encoding="utf-8")
        self.assertIsNone(FORBIDDEN_NAMES.search(svg.replace("ScarfeeOS", "")))
        root = ET.fromstring(svg)
        self.assertTrue(root.tag.endswith("}svg"))
        elements = [e.tag.split("}")[1] for e in root.iter()]
        for tag in ("text", "image", "script", "foreignObject", "style", "use"):
            self.assertNotIn(tag, elements)
        self.assertNotIn("href", svg)
        colors = set(re.findall(r"fill=\"(#[0-9a-fA-F]{6})\"", svg))
        self.assertTrue(1 <= len(colors) <= 2, colors)
        shapes = [e for e in root.iter() if e.tag.endswith("}rect") or e.tag.endswith("}path")]
        self.assertLessEqual(len(shapes), 12)
        self.assertTrue(all(e.tag.endswith("}rect") for e in shapes), "plain rectangles only")

    def test_logos_spec_installs_the_icon_named_in_os_release(self):
        tags, text = read_spec(LOGOS_DIR / "scarfeeos-logos.spec")
        self.assertEqual(tags["Name"], "scarfeeos-logos")
        self.assertEqual(tags["License"], "Apache-2.0")
        self.assertEqual(tags["URL"], PROJECT_URL)
        icon = read_os_release(RELEASE_DIR / "os-release")["LOGO"]
        self.assertIn("%{_datadir}/icons/hicolor/scalable/apps/" + icon + ".svg", text)
        self.assertIsNone(FORBIDDEN_NAMES.search(tags["Summary"]))
        for name in re.findall(r"^Source\d+:\s*(\S+)", text, re.MULTILINE):
            self.assertTrue((LOGOS_DIR / name).is_file(), name)

    @unittest.skipUnless(shutil.which("rpmbuild") and shutil.which("rpm"), "rpmbuild is not installed")
    def test_specs_build_into_packages(self):
        expected = {
            "scarfeeos-release": (RELEASE_DIR, ("system-release", "scarfeeos-release", "system-release(44)")),
            "scarfeeos-logos": (LOGOS_DIR, ("scarfeeos-logos",)),
            "scarfeeos-repos": (REPOS_DIR, ("scarfeeos-repos(44)",)),
        }
        with tempfile.TemporaryDirectory() as top:
            for name, (directory, provides) in expected.items():
                build = subprocess.run(
                    ["rpmbuild", "-bb", "--define", "_topdir " + top, "--define", "_builddir " + top + "/BUILD",
                     "--define", "_sourcedir " + str(directory), str(directory / (name + ".spec"))],
                    capture_output=True, text=True)
                self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
                rpms = list(Path(top, "RPMS", "noarch").glob(name + "-*.rpm"))
                self.assertEqual(len(rpms), 1, name)
                listed = subprocess.run(["rpm", "-qp", "--provides", str(rpms[0])],
                                        capture_output=True, text=True).stdout
                for provided in provides:
                    self.assertIn(provided, listed)

    def test_codec_repo_detector_catches_the_cisco_repository(self):
        # Guards the checks below: the repository fedora-repos enables by default must be caught.
        self.assertEqual(enabled_codec_repos(read_repo_file(FEDORA_CISCO_REPO)), ["fedora-cisco-openh264"])
        # A repository with no `enabled` key is enabled in dnf.
        self.assertEqual(enabled_codec_repos({"x": {"name": "H264 codecs"}}), ["x"])
        self.assertEqual(enabled_codec_repos({"x": {"name": "H264 codecs", "enabled": "0"}}), [])
        self.assertEqual(enabled_codec_repos({"ok": {"name": "ScarfeeOS 44", "enabled": "1"}}), [])

    def test_no_enabled_cisco_or_h264_repository_in_any_repo_file(self):
        files = sorted(HERE.rglob("*.repo"))
        self.assertIn(REPOS_DIR / "scarfeeos.repo", files)
        for path in files:
            repos = read_repo_file(path.read_text(encoding="utf-8"))
            self.assertEqual(enabled_codec_repos(repos), [], str(path))

    def test_no_cisco_or_h264_repository_in_image_description(self):
        for repo in self.root.findall("repository"):
            for field in (repo.get("alias", ""), repo.find("source").get("path")):
                self.assertIsNone(CODEC_REPO_PATTERN.search(field), field)

    def test_repos_spec_ships_only_the_scarfeeos_repo_file(self):
        tags, text = read_spec(REPOS_DIR / "scarfeeos-repos.spec")
        self.assertEqual(tags["Name"], "scarfeeos-repos")
        self.assertEqual(tags["License"], "Apache-2.0")
        self.assertEqual(tags["URL"], PROJECT_URL)
        self.assertIsNone(FORBIDDEN_NAMES.search(tags["Summary"]))
        self.assertIsNone(FORBIDDEN_NAMES.search(text.split("%description")[1].split("%prep")[0]))
        # Signing keys come from Fedora's key package; fedora-repos (which enables the Cisco repository) must not coexist.
        self.assertIn("Requires:       fedora-gpg-keys >= %{version}\n", text)
        self.assertIn("Conflicts:      fedora-repos\n", text)
        files = [l for l in text.split("%files")[1].split("%changelog")[0].splitlines() if "/" in l]
        self.assertEqual(files, ["%config(noreplace) %{_sysconfdir}/yum.repos.d/scarfeeos.repo"])
        for name in re.findall(r"^Source\d+:\s*(\S+)", text, re.MULTILINE):
            self.assertTrue((REPOS_DIR / name).is_file(), name)

    def test_repo_file_has_own_ids_and_checks_signatures(self):
        repos = read_repo_file((REPOS_DIR / "scarfeeos.repo").read_text(encoding="utf-8"))
        self.assertEqual(sorted(repos), ["scarfeeos-base", "scarfeeos-updates"])
        for repo_id, options in repos.items():
            self.assertIsNone(FORBIDDEN_NAMES.search(repo_id), repo_id)
            self.assertIsNone(FORBIDDEN_NAMES.search(options["name"]), options["name"])
            self.assertTrue(options["name"].startswith("ScarfeeOS "), options["name"])
            self.assertEqual(options["enabled"], "1")
            self.assertEqual(options["gpgcheck"], "1")
            self.assertEqual(options["gpgkey"], GPG_KEY_PATH)
            self.assertNotIn("countme", options)
            self.assertTrue(options["metalink"].startswith("https://mirrors.fedoraproject.org/metalink?repo="), repo_id)

    def test_image_installs_scarfeeos_repos_not_fedora_repos(self):
        installed = self.config["bootstrap"] + self.config["image"]
        self.assertIn("scarfeeos-repos", installed)
        for name in ("fedora-repos", "fedora-repos-rawhide", "fedora-repos-archive"):
            self.assertNotIn(name, installed)
            self.assertIn(name, self.config["ignore"])

    def test_desktop_session_packages(self):
        installed = set(self.config["image"])
        self.assertTrue({"cage", "webkit2gtk4.1"} <= installed)
        # a display manager or a full GNOME/KDE session is a different decision (README.md, "Desktop session")
        for name in installed:
            self.assertFalse(name.startswith(("gdm", "sddm", "gnome-", "plasma-", "kwin", "lightdm")), name)

    def test_no_fedora_trademark_in_image_identity(self):
        self.assertNotIn("fedora", self.root.get("name").lower())
        spec = self.root.find("description/specification").text.lower()
        self.assertNotIn("fedora", spec)
        self.assertEqual(self.root.find("preferences/type").get("volid"), "SCARFEEOS")


if __name__ == "__main__":
    unittest.main()
