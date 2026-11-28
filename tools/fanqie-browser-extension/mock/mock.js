document.getElementById("chapter-form").addEventListener("submit", event => {
  event.preventDefault();
  const ready = ["number","title","body"].every(id => !!document.getElementById(id).value.trim());
  document.getElementById("result").textContent = ready
    ? "已手动确认模拟稿件；没有联网，也没有向番茄提交。"
    : "还有空白字段，未确认。";
});
