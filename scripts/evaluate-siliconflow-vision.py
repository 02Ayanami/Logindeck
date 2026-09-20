#!/usr/bin/env python3
"""Local, synthetic-only vision evaluation. Requires Pillow; never writes API keys."""
from __future__ import annotations

import argparse
import base64
import datetime as dt
import json
import re
from pathlib import Path
import secrets
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from PIL import Image, ImageDraw, ImageFont

MODEL = "Qwen/Qwen3.5-4B"
ENDPOINT = "https://api.siliconflow.cn/v1/chat/completions"
PROVIDER = "siliconflow"
FIELDS = ("username", "password", "submit")
STATES = ("login_form", "qr_login", "verification", "already_logged_in", "unknown")
FONT = "/System/Library/Fonts/STHeiti Light.ttc"
SCHEMA = {
    "type": "object", "additionalProperties": False,
    "required": ["screen_state", *FIELDS],
    "properties": {
        "screen_state": {"type": "string", "enum": list(STATES)},
        **{field: {"anyOf": [
            {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 1000},
             "minItems": 4, "maxItems": 4}, {"type": "null"}]} for field in FIELDS},
    },
}
PROMPT = """Inspect this synthetic desktop window. Return only one JSON object with exactly
screen_state, username, password, submit. screen_state must be one of login_form,
qr_login, verification, already_logged_in, unknown. Registration is unknown, NOT login.
Only login_form may have non-null controls. username/password are the entire editable
input rectangles; submit is the entire login button, NOT labels or password-mode links.
Each rectangle is [left,top,right,bottom], integer coordinates normalized independently
to 0..1000 over the entire image. For all other states set all three controls to null.
Do not treat a verification-code input as a password. Do not return markdown or prose.
Do not perform actions. If you cannot identify a login form, return unknown with nulls.
"""


def fixtures(output: Path) -> list[dict]:
    output.mkdir(parents=True, exist_ok=True)
    cases = []
    for ident, title, size, state, dark in [
        ("login-zh", "中文登录", (800, 600), "login_form", False),
        ("login-en-dark", "英文深色登录", (1000, 640), "login_form", True),
        ("qr", "扫码登录", (800, 600), "qr_login", False),
        ("verification", "短信验证码", (800, 600), "verification", False),
        ("signed-in", "已登录主界面", (900, 600), "already_logged_in", False),
        ("registration", "注册页面", (800, 600), "unknown", False),
    ]:
        width, height = size
        bg, fg, line = ("#171b25", "#edf2ff", "#6b7892") if dark else ("#f3f5f9", "#172034", "#a7b3c8")
        im = Image.new("RGB", size, bg)
        draw = ImageDraw.Draw(im)

        def text(x, y, value, pts=22, color=None):
            draw.text((x, y), value, fill=color or fg, font=ImageFont.truetype(FONT, pts))

        def box(rect, label="", button=False):
            draw.rounded_rectangle(rect, radius=9, fill="#2861d9" if button else ("#252d3c" if dark else "white"),
                                   outline="#2861d9" if button else line, width=2)
            if label:
                text(rect[0] + 14, rect[1] + 12, label, 19, "white" if button else line)

        draw.rectangle((0, 0, width, 44), fill="#272e3c" if dark else "#e3e8f1")
        text(18, 11, "LoginDeck 测试应用 · 全部内容为虚构", 16)
        expected = {field: None for field in FIELDS}
        if ident in ("login-zh", "registration", "login-en-dark"):
            english = ident == "login-en-dark"
            left, right = (530, 900) if english else (220, 580)
            if english:
                text(70, 180, "TEST APP", 39)
                text(70, 240, "Synthetic login window", 20)
            text(left, 112, "Sign in" if english else ("注册新账号" if ident == "registration" else "账号密码登录"), 31)
            text(left, 196, "Email or username" if english else "账号", 18)
            username = (left, 229, right, 278)
            password = (left, 326, right, 375)
            submit = (left, 428, right, 480)
            box(username, "Enter username" if english else "请输入账号")
            text(left, 294, "Password" if english else "密码", 18)
            box(password, "Enter password" if english else "请输入密码")
            box(submit, "Sign in" if english else ("注册" if ident == "registration" else "登录"), True)
            if state == "login_form":
                expected = dict(username=username, password=password, submit=submit)
            else:
                text(left, 510, "已有账号？前往登录", 18)
        elif ident == "qr":
            text(238, 112, "请使用手机扫码登录", 29)
            draw.rectangle((292, 195, 508, 411), fill="white", outline=line, width=2)
            # A decorative, nonfunctional pattern: it contains no encoded URL or account.
            for row in range(17):
                for col in range(17):
                    if (row * 13 + col * 7 + row * col) % 5 < 2:
                        x, y = 305 + col * 11, 208 + row * 11
                        draw.rectangle((x, y, x + 8, y + 8), fill="#111111")
            text(284, 437, "虚构二维码 · 无法扫码", 18)
            box((275, 491, 525, 538), "切换到密码登录")
        elif ident == "verification":
            text(235, 130, "安全验证", 32)
            text(235, 203, "请输入手机收到的短信验证码", 20)
            box((235, 268, 565, 322), "六位验证码")
            box((235, 376, 565, 428), "验证", True)
        else:
            text(36, 80, "欢迎，测试用户", 26)
            draw.rectangle((24, 142, 250, 556), fill="#e4eafa")
            text(45, 166, "消息", 23)
            text(45, 225, "虚构联系人 A", 18)
            text(290, 160, "测试会话", 24)
            box((295, 226, 780, 282), "这是一条虚构消息，仅供界面测试。")
            box((645, 500, 865, 550), "退出当前账号")
        filename = ident + ".png"
        im.save(output / filename)
        cases.append({"id": ident, "title": title, "image": filename, "size": list(size),
                      "state": state, "boxes": expected})
    (output / "fixtures.json").write_text(json.dumps(cases, ensure_ascii=False, indent=2) + "\n")
    return cases


def evaluate_prediction(content: str, case: dict) -> dict:
    try:
        prediction = json.loads(content)
        if not isinstance(prediction, dict) or set(prediction) != {"screen_state", *FIELDS}:
            raise ValueError("JSON 字段不符合约定")
        if prediction["screen_state"] not in STATES:
            raise ValueError("未知页面类型")
        for field in FIELDS:
            rect = prediction[field]
            if rect is not None and (not isinstance(rect, list) or len(rect) != 4
                    or any(type(v) is not int or not 0 <= v <= 1000 for v in rect)
                    or rect[0] >= rect[2] or rect[1] >= rect[3]):
                raise ValueError("无效坐标")
        if prediction["screen_state"] != "login_form" and any(prediction[f] is not None for f in FIELDS):
            raise ValueError("非登录页不应返回填写位置")
        if prediction["screen_state"] == "login_form" and any(prediction[f] is None for f in FIELDS):
            raise ValueError("登录页缺少填写位置")
    except (ValueError, TypeError) as error:
        return {"passed": False, "format_valid": False, "reason": str(error)[:180]}
    matched = prediction["screen_state"] == case["state"]
    measurements = {}
    if matched and case["state"] == "login_form":
        width, height = case["size"]
        for field in FIELDS:
            p = prediction[field]
            a = [p[0] * width / 1000, p[1] * height / 1000, p[2] * width / 1000, p[3] * height / 1000]
            b = case["boxes"][field]
            intersection = max(0, min(a[2], b[2]) - max(a[0], b[0])) * max(0, min(a[3], b[3]) - max(a[1], b[1]))
            union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - intersection
            iou = intersection / union if union else 0
            cx, cy = (a[0] + a[2]) / 2, (a[1] + a[3]) / 2
            inside = b[0] + 4 <= cx <= b[2] - 4 and b[1] + 4 <= cy <= b[3] - 4
            measurements[field] = {"iou": round(iou, 3), "center_inside": inside, "passed": inside and iou >= 0.5}
    return {"passed": matched and all(v["passed"] for v in measurements.values()),
            "format_valid": True, "state_matched": matched, "prediction": prediction,
            "controls": measurements,
            "unsafe_fill_prediction": case["state"] != "login_form" and prediction["screen_state"] == "login_form"}


PAGE = r"""<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>LoginDeck · 免费视觉模型测试</title><style>
body{font:16px system-ui;background:#f3f5f9;color:#18243b;margin:0;padding:32px}main{max-width:960px;margin:auto}
h1{font-size:28px}p{line-height:1.7}.card{background:white;padding:24px;border-radius:16px;margin:20px 0}
input{box-sizing:border-box;width:100%;padding:13px;border:1px solid #a5b3cb;border-radius:8px;font-size:16px}
button{background:#2861d9;color:white;border:0;border-radius:8px;padding:13px 20px;margin-top:16px;font-size:16px;cursor:pointer}
button:disabled{opacity:.5}.grid{display:grid;grid-template-columns:repeat(3,1fr);gap:14px}img{width:100%;border-radius:8px;border:1px solid #d8dfeb}
small{color:#51617a}pre{white-space:pre-wrap;overflow-wrap:anywhere;font:14px system-ui;line-height:1.8}
</style><main><h1>免费视觉模型测试</h1><p>硅基流动 · Qwen3.5-4B<br>测试识别准确性，不控制 QQ，不执行登录。</p>
<section class="card"><p>下方 6 张图片均为程序生成的虚构页面。只会将这些图片发往 <strong>api.siliconflow.cn</strong>，最多 8 次请求，不切换其他模型。</p>
<label for="key">硅基流动 API 密钥</label><input id="key" type="password" autocomplete="off" spellcheck="false" placeholder="在此粘贴，内容隐藏且不会保存">
<small>密钥仅供本次测试使用，不写入文件或日志。测试输出不包含密钥。</small><br>
<button id="run">开始测试</button><p id="status" role="status">等待输入密钥。</p><pre id="result"></pre></section>
<section class="card"><h2>将发送的测试图片</h2><div class="grid">__IMAGES__</div></section>
<p>坐标按图片宽高归一化至 0–1000；登录控件要求中心点位于实际控件内部，且矩形重合度至少 50%。
非登录页面必须识别正确且不返回填写位置。样本通过不代表已适配真实 QQ。</p></main>
<script>
const base=location.pathname.replace(/\/$/,'');let polling=false;
async function refresh(){try{const r=await fetch(base+'/status');const s=await r.json();document.querySelector('#status').textContent=s.message;
document.querySelector('#run').disabled=s.running;if(s.results.length)document.querySelector('#result').textContent=s.results.map(x=>(x.passed?'✓ ':'✗ ')+x.title+'：'+(x.reason||(x.passed?'通过':'未通过'))).join('\n');
if(s.finished){polling=false;document.querySelector('#run').disabled=true;}else if(polling)setTimeout(refresh,1200);}catch{document.querySelector('#status').textContent='本地测试服务连接中断。';polling=false;}}
document.querySelector('#run').onclick=async()=>{const input=document.querySelector('#key');const key=input.value.trim();if(!key){document.querySelector('#status').textContent='请先填写 API 密钥。';return;}
input.value='';document.querySelector('#run').disabled=true;document.querySelector('#status').textContent='开始测试…';
try{const r=await fetch(base+'/run',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({key})});
const s=await r.json();if(!r.ok)throw new Error(s.message);polling=true;refresh();}catch(e){document.querySelector('#status').textContent=e.message;document.querySelector('#run').disabled=false;}};
refresh();</script></html>"""


class Probe:
    def __init__(self, output: Path, proxy: str | None, manifest: Path | None = None):
        self.output = output
        self.synthetic_only = manifest is None
        if manifest is None:
            self.cases = fixtures(output)
        else:
            self.output.mkdir(parents=True, exist_ok=True)
            self.cases = json.loads(manifest.read_text())
            for case in self.cases:
                if Path(case["image"]).name != case["image"]:
                    raise ValueError("Image must be a filename")
                with Image.open(manifest.parent / case["image"]) as im:
                    if list(im.size) != case["size"]:
                        raise ValueError("Image dimensions mismatch")
                    im.convert("RGB").save(output / case["image"])
            (output / "fixtures.json").write_text(json.dumps(self.cases, ensure_ascii=False, indent=2) + "\n")
        self.lock = threading.Lock()
        self.state = {"running": False, "finished": False, "message": "等待输入密钥。", "results": []}
        handlers = [urllib.request.ProxyHandler({"https": proxy})] if proxy else [urllib.request.ProxyHandler({})]
        # Never forward an Authorization header across redirects.
        class NoRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, req, fp, code, msg, headers, newurl):
                return None
        self.client = urllib.request.build_opener(*handlers, NoRedirect())

    def request(self, key: str, case: dict, mode: str):
        image = base64.b64encode((self.output / case["image"]).read_bytes()).decode()
        width, height = case["size"]
        dimensions = (f"\nThe complete image is {width} pixels wide and {height} pixels high. "
                      "The origin is the top-left corner INCLUDING the title bar. "
                      f"Normalize x by dividing pixel x by {width} and multiplying by 1000; "
                      f"normalize y by dividing pixel y by {height} and multiplying by 1000. "
                      "Identify the actual visible rectangle edges; do not infer a conventional layout.")
        body = {"model": MODEL, "stream": False, "temperature": 0, "max_tokens": 1200, "enable_thinking": False,
                "messages": [{"role": "user", "content": [
                    {"type": "text", "text": PROMPT.replace("synthetic desktop window", "desktop window") + dimensions},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64," + image}}]}]}
        if mode == "json_schema":
            body["response_format"] = {"type": "json_schema", "json_schema": {"name": "login_window", "strict": True, "schema": SCHEMA}}
        elif mode == "json_object":
            body["response_format"] = {"type": "json_object"}
        if PROVIDER == "openrouter":
            body.pop("enable_thinking")
            body["reasoning"] = {"enabled": False}
            body["provider"] = {"require_parameters": True, "data_collection": "deny", "allow_fallbacks": False}
        elif PROVIDER == "google":
            config = {"temperature": 0, "maxOutputTokens": 8192}
            if mode != "prompt_json":
                config["responseMimeType"] = "application/json"
            if mode == "json_schema":
                config["responseJsonSchema"] = SCHEMA
            body = {"contents": [{"role": "user", "parts": [
                {"text": PROMPT.replace("synthetic desktop window", "desktop window") + dimensions},
                {"inlineData": {"mimeType": "image/png", "data": image}}]}],
                "generationConfig": config}
        headers = {"Content-Type": "application/json"}
        headers["x-goog-api-key" if PROVIDER == "google" else "Authorization"] = key if PROVIDER == "google" else "Bearer " + key
        req = urllib.request.Request(ENDPOINT, data=json.dumps(body).encode(), method="POST",
                                     headers=headers)
        with self.client.open(req, timeout=90) as response:
            data = json.loads(response.read(2_000_000))
        if PROVIDER == "google":
            candidates = data.get("candidates") or []
            if not candidates:
                raise RuntimeError("Google 未返回识别结果，可能被内容过滤或生成失败。")
            candidate = candidates[0]
            content = "".join(p.get("text", "") for p in candidate.get("content", {}).get("parts", []) if not p.get("thought"))
            usage = data.get("usageMetadata") or {}
            data = {"choices": [{"message": {"content": content}, "finish_reason": candidate.get("finishReason")}],
                    "usage": {"prompt_tokens": usage.get("promptTokenCount"),
                              "completion_tokens": usage.get("candidatesTokenCount"), "total_tokens": usage.get("totalTokenCount")}}
        return data

    def run(self, key: str):
        mode = "json_schema"
        requests = 0
        attempts = []
        started = dt.datetime.now(dt.timezone.utc).isoformat()
        try:
            for index, case in enumerate(self.cases):
                with self.lock:
                    self.state["message"] = f"正在测试 {index + 1}/{len(self.cases)}：{case['title']}（单张最多等待 90 秒）"
                data = None
                request_error = None
                transport_retries = 0
                while True:
                    begin = time.monotonic()
                    requests += 1
                    try:
                        data = self.request(key, case, mode)
                        break
                    except urllib.error.HTTPError as error:
                        message = error.read(8192).decode("utf-8", "replace").replace(key, "[REDACTED]")
                        try:
                            detail = json.loads(message).get("error", {})
                            detail = detail.get("message", "") if isinstance(detail, dict) else ""
                        except (ValueError, AttributeError):
                            detail = "服务返回了非 JSON 错误页面"
                        detail = re.sub(r'(?:AIza|sk-)[A-Za-z0-9_-]{12,}', '[REDACTED]', str(detail))
                        detail = re.sub(r'\b[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}\b', '[REDACTED_EMAIL]', detail)
                        detail = re.sub(r'projects/[A-Za-z0-9_-]+', 'projects/[REDACTED]', detail)[:700]
                        attempts.append({"case": case["id"], "mode": mode, "http_status": error.code, "service_message": detail})
                        format_error = error.code in (400, 422) and any(term in message.lower() for term in
                            ("response_format", "json_schema", "json_object", "structured", "json mode", "responsejsonschema", "response_json_schema"))
                        if index == 0 and mode != "prompt_json" and format_error:
                            mode = "json_object" if mode == "json_schema" else "prompt_json"
                            continue
                        if error.code >= 500:
                            request_error = f"接口暂时不可用（HTTP {error.code}），此项尚未验证"
                            break
                        hints = {400: "请求参数或账号使用条件不被接受", 401: "密钥鉴权失败",
                                 402: "服务要求付费或余额不足；不会自动启用付费",
                                 403: "当前账号、项目权限或地区不允许调用",
                                 404: "模型或接口不可用", 429: "免费额度或请求速率受限"}
                        hint = hints.get(error.code, "请检查服务配置和账号权限")
                        raise RuntimeError(f"接口返回 HTTP {error.code}：{hint}；已停止。服务说明：{detail}") from None
                    except (TimeoutError, urllib.error.URLError) as error:
                        cause = error.reason if isinstance(error, urllib.error.URLError) else error
                        category = "请求等待超时" if isinstance(cause, TimeoutError) else "网络连接失败"
                        detail = str(cause).replace(key, "[REDACTED]")[:200]
                        attempts.append({"case": case["id"], "mode": mode, "transport_error": category, "detail": detail})
                        if transport_retries == 0:
                            transport_retries += 1
                            with self.lock:
                                self.state["message"] = f"{category}，15 秒后重试一次：{case['title']}"
                            time.sleep(15)
                            continue
                        request_error = f"{category}，重试后仍未取得结果，此项尚未验证"
                        break
                elapsed = round(time.monotonic() - begin, 2)
                try:
                    if data is None:
                        raise ValueError("No inference response")
                    content = data["choices"][0]["message"]["content"]
                    if not isinstance(content, str):
                        raise ValueError("模型未返回文本 JSON")
                    content = content.replace(key, "[REDACTED]")
                    result = evaluate_prediction(content, case)
                    result["response_text"] = content[:12000]
                    result["finish_reason"] = data["choices"][0].get("finish_reason")
                    result["usage"] = {k: v for k, v in (data.get("usage") or {}).items()
                                       if k in ("prompt_tokens", "completion_tokens", "total_tokens") and type(v) is int}
                except (KeyError, IndexError, TypeError, ValueError):
                    result = {"passed": False, "reason": request_error or "接口响应缺少可解析的模型文本", "inconclusive": True}
                result.update({"id": case["id"], "title": case["title"], "seconds": elapsed, "mode": mode})
                with self.lock:
                    self.state["results"].append(result)
                # Keep the probe below 20 requests/minute even if responses are unusually fast.
                if index < len(self.cases) - 1:
                    time.sleep(max(0, (15 if PROVIDER == "google" else 3.2) - (time.monotonic() - begin)))
            with self.lock:
                passed = sum(result["passed"] for result in self.state["results"])
                incomplete = sum(bool(result.get("inconclusive")) for result in self.state["results"])
                failed = len(self.state["results"]) - passed - incomplete
                self.state["message"] = f"测试完成：{passed} 项通过，{failed} 项未通过，{incomplete} 项未完成验证。真实 QQ 兼容性仍需另测。"
        except Exception as error:
            message = str(error).replace(key, "[REDACTED]")[:300]
            with self.lock:
                self.state["message"] = "测试停止：" + message
        finally:
            key = ""
            with self.lock:
                self.state.update(running=False, finished=True)
                report = {"started_at": started, "model": MODEL, "endpoint": ENDPOINT,
                          "synthetic_only": self.synthetic_only, "provider": PROVIDER, "enable_thinking": "model_default" if PROVIDER == "google" else False, "explicit_image_dimensions": True,
                          "request_count": requests, "format_attempts": attempts,
                          "message": self.state["message"], "results": self.state["results"]}
                (self.output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
            print(report["message"], flush=True)
            print("Report: " + str(self.output / "report.json"), flush=True)


def serve(probe: Probe, port: int):
    token = secrets.token_urlsafe(24)
    prefix = "/" + token
    images = "".join(f'<div><img src="{prefix}/{c["image"]}"><small>{c["title"]}</small></div>' for c in probe.cases)
    page_text = PAGE.replace("__IMAGES__", images)
    page_text = page_text.replace("下方 6 张", f"下方 {len(probe.cases)} 张").replace("最多 8 次请求", f"最多 {len(probe.cases) * 2 + 2} 次请求")
    if PROVIDER == "openrouter":
        page_text = page_text.replace("硅基流动", "OpenRouter").replace("Qwen3.5-4B", "Nex N2.5 Mini · free")
        page_text = page_text.replace("api.siliconflow.cn", "openrouter.ai（转发给该模型的推理服务商）")
    elif PROVIDER == "google":
        page_text = page_text.replace("硅基流动", "Google Gemini").replace("Qwen3.5-4B", "Gemini 3.6 Flash")
        page_text = page_text.replace("api.siliconflow.cn", "generativelanguage.googleapis.com")
        page_text = page_text.replace('<label for="key">', '<p>请使用 Google AI Studio 免费层项目的密钥；已启用计费的项目可能按量收费。免费层内容可能用于改进产品，本次仅发送下方虚构图片。请求间隔至少 15 秒，预计需 1–3 分钟。</p><label for="key">')
    if not probe.synthetic_only:
        page_text = page_text.replace("均为程序生成的虚构页面", "为你提供的 QQ 登录窗口截图，账号密码为空")
        page_text = page_text.replace("本次仅发送下方虚构图片", "本次仅发送下方预览截图")
        page_text = page_text.replace("真实 QQ 兼容性仍需另测", "本次只验证截图定位，不代表自动填写通过")
        page_text = page_text.replace('>开始测试</button>', '>确认发送预览截图并开始测试</button>')
    page = page_text.encode()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def send(self, status, body, content_type="application/json; charset=utf-8"):
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("Referrer-Policy", "no-referrer")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Content-Security-Policy", "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; form-action 'none'")
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.headers.get("Host") != f"127.0.0.1:{self.server.server_port}":
                return self.send(403, b'{}')
            if self.path in (prefix, prefix + "/"):
                return self.send(200, page, "text/html; charset=utf-8")
            if self.path == prefix + "/status":
                with probe.lock:
                    body = json.dumps(probe.state, ensure_ascii=False).encode()
                return self.send(200, body)
            match = next((case for case in probe.cases if self.path == prefix + "/" + case["image"]), None)
            if match:
                return self.send(200, (probe.output / match["image"]).read_bytes(), "image/png")
            self.send(404, b'{}')

        def do_POST(self):
            origin = f"http://127.0.0.1:{self.server.server_port}"
            if (self.path != prefix + "/run" or self.headers.get("Origin") != origin
                    or self.headers.get("Host") != f"127.0.0.1:{self.server.server_port}"
                    or self.headers.get("Content-Type") != "application/json"):
                return self.send(403, b'{"message":"Invalid local origin"}')
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= 8192:
                    raise ValueError()
                payload = json.loads(self.rfile.read(length))
                key = payload.pop("key").strip()
                if not key or len(key) > 4096 or any(ord(c) < 33 or ord(c) > 126 for c in key):
                    raise ValueError()
            except (ValueError, TypeError, KeyError, AttributeError):
                return self.send(400, b'{"message":"Invalid API key input"}')
            with probe.lock:
                if probe.state["running"] or probe.state["finished"]:
                    return self.send(409, b'{"message":"This evaluation has already started"}')
                probe.state.update(running=True, message="正在连接模型服务…")
            threading.Thread(target=probe.run, args=(key,), daemon=True).start()
            key = ""
            self.send(202, b'{"accepted":true}')

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"URL: http://127.0.0.1:{server.server_port}{prefix}/", flush=True)
    print("Only the previewed test images will be sent to " + ENDPOINT, flush=True)
    server.serve_forever()


def main():
    global PROVIDER, MODEL, ENDPOINT
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--proxy", help="Optional HTTPS CONNECT proxy for the official API")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--provider", choices=("siliconflow", "openrouter", "google"), default="siliconflow")
    parser.add_argument("--cases", nargs="+", choices=("login-zh", "login-en-dark", "qr", "verification", "signed-in", "registration"))
    parser.add_argument("--manifest", type=Path, help="Local reviewed image fixture manifest")
    args = parser.parse_args()
    PROVIDER = args.provider
    if PROVIDER == "openrouter":
        MODEL = "nex-agi/nex-n2.5-mini:free"
        ENDPOINT = "https://openrouter.ai/api/v1/chat/completions"
    elif PROVIDER == "google":
        MODEL = "gemini-3.6-flash"
        ENDPOINT = "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.6-flash:generateContent"
    probe = Probe(args.output.resolve(), args.proxy, args.manifest)
    if args.cases:
        probe.cases = [case for case in probe.cases if case["id"] in args.cases]
        (probe.output / "fixtures.json").write_text(json.dumps(probe.cases, ensure_ascii=False, indent=2) + "\n")
    if args.prepare_only:
        for case in probe.cases:
            width, height = case["size"]
            prediction = {"screen_state": case["state"], **{field: (
                [round(v * 1000 / (width if i % 2 == 0 else height)) for i, v in enumerate(case["boxes"][field])]
                if case["boxes"][field] else None) for field in FIELDS}}
            assert evaluate_prediction(json.dumps(prediction), case)["passed"], case["id"]
        print(f"Prepared {len(probe.cases)} {'synthetic' if probe.synthetic_only else 'reviewed image'} cases; ground-truth evaluator checks passed.")
    else:
        serve(probe, args.port)


if __name__ == "__main__":
    main()
