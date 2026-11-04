#!/usr/bin/env python3
"""Extract one stable release's notes from CHANGELOG.md."""
import argparse
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent
STABLE = r'(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)'

def extract(changelog: str, version: str) -> str:
    if not re.fullmatch(STABLE, version):
        raise ValueError('Release notes require a stable MAJOR.MINOR.PATCH version')
    entries=list(re.finditer(r'^## \[([^\]\n]+)\][^\n]*\n',changelog,re.MULTILINE))
    versions=[entry.group(1) for entry in entries]
    if len(re.findall(r'^## ',changelog,re.MULTILINE)) != len(entries):
        raise ValueError('CHANGELOG headings must identify stable releases')
    if any(not re.fullmatch(STABLE,value) for value in versions) or len(set(versions)) != len(versions):
        raise ValueError('CHANGELOG must contain unique stable release sections only')
    if version not in versions:
        raise ValueError(f'Missing changelog section: {version}')
    index=versions.index(version)
    start=entries[index].end()
    end=entries[index+1].start() if index+1<len(entries) else len(changelog)
    body=changelog[start:end].strip()
    if not body:
        raise ValueError(f'Empty changelog section: {version}')
    body=re.sub(r'\]\(((?:docs|macos)/[^\s)]+)\)',
                lambda match: f'](https://github.com/LoveElysia1314/sysu-netauth/blob/v{version}/{match.group(1)})', body)
    return f'SYSU NetAuth v{version}\n\n{body}\n\n[完整更新日志](https://github.com/LoveElysia1314/sysu-netauth/blob/main/CHANGELOG.md)\n'

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--version',required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    notes=extract((ROOT/'CHANGELOG.md').read_text(encoding='utf-8'),args.version)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(notes,encoding='utf-8')
    print(args.output)

if __name__=='__main__': main()
