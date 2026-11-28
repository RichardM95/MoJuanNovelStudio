"""Write a cleaned novel TXT and its source-bound review ledger."""
import argparse
import codecs
import hashlib
import json
import re
from pathlib import Path


HEADING = re.compile(
    r"^(?:#{1,6}\s+|--\s*)?(?:(?:正文\s*)?第\s*[零〇一二两三四五六七八九十百千万\d]+\s*"
    r"[卷部篇章节節回集话話]|(?:chapter|volume|part|book)\s+\d+\b|"
    r"(?:序章|楔子|番外|后记|後記|尾声|引子)(?:$|[\s：:（(]))", re.I
)
LABEL = re.compile(r"^(?:作者有话说|作者的话|作者说|作家的话|写在章末)(?:\s*[:：]\s*|$)")
SIGNAL = re.compile(
    r"作者有话说|作者的话|作家的话|求.{0,3}(?:推荐票|月票|订阅|收藏)|更新通知|"
    r"书友群|请假条|请假一天|加更|每天.{0,3}更|上架感言|本书首发|最新网址|"
    r"手机用户请|请记住本站|https?://|www\.", re.I
)
AD = re.compile(
    r"(?:最新网址|本站网址|请记住本站(?:域名|网址)|手机用户请(?:到|访问))\s*[:：]?\s*"
    r"(?:(?:https?://|www\.)[A-Za-z0-9./_?=%&#:-]+|[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+)"
    r"(?:\s*(?:阅读|浏览))?[。！!]?", re.I
)
VOTE = re.compile(
    r"(?:求(?:推荐票|月票|订阅|收藏)[！!。，、\s]*)+"
    r"(?:诸位手上有的话就砸过来吧[！!。]?)?"
)
UPDATE = re.compile(r"(?:(?:更新通知|请假条)\s*[:：]\s*)?"
                    r"(?:今天|今日|明天|明日|本章|本周)(?:请假一天|停更一天|暂停更新|恢复更新|加更一章)[。！!]?")
WRAPPERS = {"（": "）", "(": ")", "【": "】"}
DIALOGUE = re.compile("[“”「」『』\"《》]")
PHYSICAL_LINE = re.compile(r"[^\r\n]*(?:\r\n|\r|\n)|[^\r\n]+$")


def decode(raw, requested):
    for bom, codec in (
        (codecs.BOM_UTF32_LE, "utf-32-le"), (codecs.BOM_UTF32_BE, "utf-32-be"),
        (codecs.BOM_UTF8, "utf-8"), (codecs.BOM_UTF16_LE, "utf-16-le"),
        (codecs.BOM_UTF16_BE, "utf-16-be"),
    ):
        if raw.startswith(bom):
            break
    else:
        bom = b""
        if requested:
            codec = codecs.lookup(requested).name
        elif b"\0" in raw:
            raise ValueError("检测到空字节，请指定原编码及字节序")
        else:
            try:
                return raw.decode("utf-8"), "utf-8", bom
            except UnicodeDecodeError:
                codec = "gb18030"
    if requested and codecs.lookup(requested).name != codec:
        raise ValueError("指定编码与 BOM 冲突")
    if codec not in {"utf-8", "utf-16-le", "utf-16-be", "utf-32-le", "utf-32-be",
                     "gbk", "gb18030", "gb2312", "big5", "cp950"}:
        raise ValueError(f"不支持该编码：{codec}")
    body = raw[len(bom):]
    text = body.decode(codec)
    if text.encode(codec) != body:
        raise ValueError("编码无法逐字节往返，请核实源文件编码")
    return text, codec, bom


def classify(text, at_end):
    inner = text
    if text and WRAPPERS.get(text[0]) == text[-1]:
        inner = text[1:-1].strip()
    label = LABEL.match(inner)
    content = inner[label.end():].strip() if label else inner
    if AD.fullmatch(content):
        return "remove", "独立站点广告"
    if (at_end or label) and (VOTE.fullmatch(content) or UPDATE.fullmatch(content)):
        return "remove", "独立求票或更新通知"
    if label or SIGNAL.search(inner):
        return "keep", "疑似附言、推广或正文混排"
    return None


def scan(lines):
    at_end = True
    endings = set()
    for i in range(len(lines) - 1, -1, -1):
        text = lines[i].strip()
        if not text or re.fullmatch(r"[-=*─—]{3,}", text):
            continue
        if at_end:
            endings.add(i)
        at_end = bool(HEADING.match(text))

    entries = []
    chapter = "卷章标题前"
    in_note = False
    for i, line in enumerate(lines):
        text = line.strip()
        if HEADING.match(text):
            chapter, in_note = text, False
            continue
        if not text:
            continue
        result = classify(text, i in endings)
        if LABEL.fullmatch(text):
            in_note = True
        if not result and in_note:
            result = "keep", "作者栏之后，需核定附言结束位置"
        if result:
            action, reason = result
            if DIALOGUE.search(text):
                action, reason = "keep", "含对白或引文"
            entries.append(dict(line=i + 1, chapter=chapter, action=action, reason=reason, text=line))
    return entries


def apply_review(entries, path, source_hash, lines):
    review = json.loads(path.read_text(encoding="utf-8-sig"))
    if review["source_sha256"] != source_hash:
        raise ValueError("清单与原稿 SHA-256 不一致")
    choices = [e for e in review["entries"] if not e.get("additional")]
    if len(choices) != len(entries):
        raise ValueError("清单条数不匹配")
    for entry, choice in zip(entries, choices):
        if (choice["line"] != entry["line"] or choice["text"] != entry["text"]
                or choice["action"] not in {"keep", "remove"}):
            raise ValueError(f"清单与原稿第 {entry['line']} 行不匹配")
        if entry["action"] != choice["action"]:
            entry.update(action=choice["action"], reason="根据复核清单调整")


    extra = [e for e in review["entries"] if e.get("additional")] + review.get("additional_removals", [])
    if not extra:
        return
    seen = {e["line"] for e in entries}
    chapter = "卷章标题前"
    chapters = {}
    wanted = {e["line"] for e in extra}
    for number, line in enumerate(lines, 1):
        if HEADING.match(line.strip()):
            chapter = line.strip()
        if number in wanted:
            chapters[number] = chapter
    for item in extra:
        number = item["line"]
        if (type(number) is not int or number not in chapters or number in seen
                or lines[number - 1] != item["text"] or not item["text"].strip()
                or HEADING.match(item["text"].strip()) or not item["reason"].strip()
                or item.get("action", "remove") not in {"keep", "remove"}):
            raise ValueError(f"额外复核项无效：{number}")
        seen.add(number)
        entries.append(dict(line=number, chapter=chapters[number], additional=True,
                            action=item.get("action", "remove"), reason=item["reason"], text=item["text"]))
    entries.sort(key=lambda entry: entry["line"])


def clean(source, output_dir, encoding, review_file):
    raw = source.read_bytes()
    text, codec, bom = decode(raw, encoding)
    lines = PHYSICAL_LINE.findall(text)
    entries = scan(lines)
    source_hash = hashlib.sha256(raw).hexdigest()
    if review_file:
        apply_review(entries, review_file, source_hash, lines)
    removed = {e["line"] for e in entries if e["action"] == "remove"}
    retained = "".join(line for i, line in enumerate(lines, 1) if i not in removed)
    cleaned = bom + retained.encode(codec)
    out = output_dir / (source.stem + "（清洗版）.txt")
    report = dict(source=str(source.resolve()), source_sha256=source_hash, cleaned=out.name,
                  cleaned_sha256=hashlib.sha256(cleaned).hexdigest(), encoding=codec, bom=bool(bom),
                  lines=len(lines), removed=len(removed), kept_candidates=len(entries) - len(removed),
                  entries=entries)
    ledger = json.dumps(report, ensure_ascii=False, indent=2).encode("utf-8")
    # Reserving the directory prevents two runs from mixing output and report.
    output_dir.mkdir(parents=True, exist_ok=False)
    created = []
    try:
        for path, data in ((out, cleaned), (output_dir / "清洗清单.json", ledger)):
            with path.open("xb") as stream:
                created.append(path)
                stream.write(data)
    except OSError:
        for path in created:
            path.unlink(missing_ok=True)
        if not any(output_dir.iterdir()):
            output_dir.rmdir()
        raise
    print(f"移除 {len(removed)} 行，保留候选 {len(entries) - len(removed)} 行。结果位于 {output_dir}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output_dir", type=Path, help="新的结果目录")
    parser.add_argument("--encoding", help="可显式指定编码，如 gbk、utf-16-le")
    parser.add_argument("--review", type=Path, help="已核对 action 的清单副本")
    args = parser.parse_args()
    try:
        clean(args.source, args.output_dir, args.encoding, args.review)
    except (OSError, ValueError, LookupError, TypeError) as error:
        parser.exit(1, f"清洗失败：{error}\n")


if __name__ == "__main__":
    main()
