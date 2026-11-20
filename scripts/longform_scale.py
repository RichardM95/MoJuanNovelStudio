#!/usr/bin/env python3
"""Linux-only supervisor for the prebuilt, opt-in longform workload."""

import argparse
import ctypes
import json
import os
from pathlib import Path
import platform
import resource
import select
import shutil
import signal
import stat
import sys
import tempfile
import time

MIB = 1024 * 1024
GIB = 1024 * MIB
MAX_WALL = 30 * 60
MAX_DISK = MAX_AS = MIN_FREE = 2 * GIB
POLL_SECONDS = 0.05
REPO = Path(__file__).resolve().parents[1]


def optional_text(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return None


def hardware(run_dir):
    cpu = optional_text("/proc/cpuinfo") or ""
    model = next((line.split(":", 1)[1].strip() for line in cpu.splitlines()
                  if line.split(":", 1)[0].strip() in ("model name", "Hardware")), None)
    mem = optional_text("/proc/meminfo") or ""
    memory_kib = next((int(line.split()[1]) for line in mem.splitlines()
                       if line.startswith("MemTotal:")), None)
    disk = shutil.disk_usage(run_dir)
    membership = optional_text("/proc/self/cgroup")
    # In a unified cgroup mount, try this process's membership and its visible ancestors.
    cg_root = Path("/sys/fs/cgroup")
    v2_path = next((line.removeprefix("0::/") for line in (membership or "").splitlines()
                    if line.startswith("0::/")), "")
    roots = [cg_root / v2_path, *(cg_root / v2_path).parents]
    roots = [p for p in roots if p == cg_root or cg_root in p.parents]
    return {
        "platform": platform.platform(),
        "cpu_model": model,
        "visible_logical_cpus": os.cpu_count(),
        "affinity": sorted(os.sched_getaffinity(0)),
        "proc_mem_total_kib": memory_kib,
        "cgroup_membership": membership,
        "visible_cgroup_memory_max": [x for p in roots
                                      if (x := optional_text(p / "memory.max")) is not None] or None,
        "visible_cgroup_cpu_max": [x for p in roots
                                   if (x := optional_text(p / "cpu.max")) is not None] or None,
        "filesystem_total_bytes": disk.total,
        "filesystem_initial_free_bytes": disk.free,
    }


def child_has_exited(pid):
    info = optional_text(f"/proc/{pid}/status")
    if info is None:
        return True
    return any(line.split()[1] in {"Z", "X"} for line in info.splitlines()
               if line.startswith("State:"))


def files_under(root, child_pid=None):
    sizes = {}
    seen = set()
    logical = allocated = accounted = 0

    def charge(path, info):
        nonlocal logical, allocated, accounted
        identity = (info.st_dev, info.st_ino)
        if not stat.S_ISREG(info.st_mode) or identity in seen:
            return
        seen.add(identity)
        blocks = info.st_blocks * 512
        sizes[path] = {"logical_bytes": info.st_size, "allocated_bytes": blocks}
        logical += info.st_size
        allocated += blocks
        accounted += max(info.st_size, blocks)

    for directory, _, files in os.walk(root):
        for filename in files:
            path = Path(directory) / filename
            try:
                charge(str(path.relative_to(root)), path.lstat())
            except FileNotFoundError:  # SQLite may remove a sidecar between list/stat.
                pass

    if child_pid is not None:
        # SQLite can unlink a temp file immediately while keeping its pages on disk.
        try:
            handles = list(Path(f"/proc/{child_pid}/fd").iterdir())
        except FileNotFoundError:  # child ended since the previous wait4 poll
            handles = []
        except PermissionError:
            if not child_has_exited(child_pid):
                raise
            handles = []
        for handle in handles:
            try:
                target = os.readlink(handle)
                if target.startswith(f"{root}/") and target.endswith(" (deleted)"):
                    charge(target.removeprefix(f"{root}/"), handle.stat())
            except FileNotFoundError:
                pass
            except PermissionError:
                if not child_has_exited(child_pid):
                    raise
    return sizes, logical, allocated, accounted


def child_rss_kib(pid):
    info = optional_text(f"/proc/{pid}/status") or ""
    return next((int(line.split()[1]) for line in info.splitlines()
                 if line.startswith("VmRSS:")), 0)


def apply_limit(kind, ceiling):
    soft, hard = resource.getrlimit(kind)
    values = [ceiling, *(v for v in (soft, hard) if v != resource.RLIM_INFINITY)]
    cap = min(values)
    resource.setrlimit(kind, (cap, cap))


def launch(binary, directory, name, command, disk_limit):
    supervisor = os.getpid()
    libc = ctypes.CDLL(None, use_errno=True)
    with (directory / f"{name}.stdout.json").open("wb") as out, (
        directory / f"{name}.stderr.log"
    ).open("wb") as err:
        pid = os.fork()
        if pid == 0:
            try:
                os.setsid()
                # Linux parent-death signal is preserved over ordinary execve.
                if libc.prctl(1, signal.SIGKILL, 0, 0, 0) != 0:  # PR_SET_PDEATHSIG
                    raise OSError(ctypes.get_errno(), "cannot arm supervisor-death guard")
                if os.getppid() != supervisor:
                    os._exit(125)
                os.dup2(out.fileno(), 1)
                os.dup2(err.fileno(), 2)
                os.chdir(directory)
                apply_limit(resource.RLIMIT_AS, MAX_AS)
                apply_limit(resource.RLIMIT_FSIZE, disk_limit)
                apply_limit(resource.RLIMIT_CORE, 0)
                # prepare_restore uses std::env::temp_dir(), so keep its potentially
                # large staging DB under the guarded workspace, not system /tmp.
                os.execve(binary, [str(binary), *command], {
                    **os.environ, "TMPDIR": str(directory / "staging"),
                    "TMP": str(directory / "staging"), "TEMP": str(directory / "staging"),
                    "SQLITE_TMPDIR": str(directory / "staging"),
                })
            except BaseException as error:
                os.write(2, f"stage launcher failed: {error}\n".encode())
                os._exit(125)
    return pid


def terminate(pid):
    try:
        os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:
        # The deadline may have elapsed before the child created its session.
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def run_stage(binary, directory, name, command, deadline, disk_limit):
    started = time.monotonic()
    # React before the absolute ceilings, leaving headroom for writes during
    # a polling interval. The per-file and per-process address-space limits
    # are additionally enforced in the kernel; this is not a filesystem quota.
    margin = min(64 * MIB, disk_limit // 20)
    peak_disk = peak_logical = peak_allocated = peak_sampled_rss = 0
    min_free = shutil.disk_usage(directory).free
    pid = pidfd = None
    waited = None
    stop = None
    try:
        while True:
            _, logical, allocated, disk = files_under(directory, pid)
            min_free = min(min_free, shutil.disk_usage(directory).free)
            peak_disk = max(peak_disk, disk)
            peak_logical = max(peak_logical, logical)
            peak_allocated = max(peak_allocated, allocated)
            if pid is not None:
                peak_sampled_rss = max(peak_sampled_rss, child_rss_kib(pid))
            if time.monotonic() >= deadline:
                stop = "overall time budget reached"
            elif disk >= disk_limit - margin:
                stop = "workspace disk guard reached"
            elif min_free <= MIN_FREE + margin:
                stop = "minimum free disk space guard reached"
            elif peak_sampled_rss * 1024 >= MAX_AS:
                stop = "stage RSS limit reached"
            if stop:
                break
            if pid is None:
                pid = launch(binary, directory, name, command, disk_limit)
                try:
                    pidfd = os.pidfd_open(pid)
                except (AttributeError, OSError):
                    pass  # Older kernels still support nonblocking wait4 polling.
            else:
                waited = os.wait4(pid, os.WNOHANG)
                if waited[0]:
                    break
            delay = max(0, min(POLL_SECONDS, deadline - time.monotonic()))
            if pidfd is None:
                time.sleep(delay)
            else:
                select.select([pidfd], [], [], delay)
    except BaseException as error:
        stop = f"supervisor interrupted: {type(error).__name__}: {error}"
    finally:
        if pid is not None and not (waited and waited[0]):
            terminate(pid)
            waited = os.wait4(pid, 0)
        if pidfd is not None:
            os.close(pidfd)

    wall = time.monotonic() - started
    files_after, logical, allocated, disk = files_under(directory)
    peak_disk = max(peak_disk, disk)
    peak_logical = max(peak_logical, logical)
    peak_allocated = max(peak_allocated, allocated)
    if peak_disk >= disk_limit:
        stop = stop or "workspace disk budget reached"
    min_free = min(min_free, shutil.disk_usage(directory).free)
    if min_free <= MIN_FREE:
        stop = stop or "minimum free disk space budget reached"
    if waited is None:
        exit_code = rss = user = system = None
    else:
        _, status, usage = waited
        exit_code = os.waitstatus_to_exitcode(status)
        # Includes the child before execve, when resident launcher pages may
        # still be inherited from Python. This is not a Rust-only high-water mark.
        rss, user, system = usage.ru_maxrss, usage.ru_utime, usage.ru_stime
        if rss * 1024 > MAX_AS:
            stop = stop or "stage RSS limit exceeded"
        if exit_code != 0:
            stop = stop or f"stage process exited with status {exit_code}; inspect {name}.stderr.log"
    metrics = None
    if not stop:
        try:
            metrics = json.loads((directory / f"{name}.stdout.json").read_text())
            if not isinstance(metrics, dict):
                raise ValueError("stage output is not a JSON object")
        except (OSError, ValueError) as error:
            stop = f"invalid stage metrics: {error}"
    return {
        "name": name,
        "command": command,
        "status": "incomplete" if stop else "complete",
        "stop_reason": stop,
        "exit_code": exit_code,
        "wall_seconds": wall,
        "user_cpu_seconds": user,
        "system_cpu_seconds": system,
        "peak_rss_kib": rss,
        "sampled_peak_rss_kib": peak_sampled_rss,
        "sampled_run_tree_charged_peak_bytes": peak_disk,
        "sampled_run_tree_logical_peak_bytes": peak_logical,
        "sampled_run_tree_allocated_peak_bytes": peak_allocated,
        "sampled_filesystem_min_free_bytes": min_free,
        "files_after": files_after,
        "metrics": metrics,
    }


def write_report(path, report):
    staging = path.with_suffix(".partial")
    staging.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    staging.replace(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path, help="prebuilt Rust harness")
    parser.add_argument("--profile", required=True, choices=["dev", "release"],
                        help="label for the profile actually used to build --binary")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--smoke", action="store_true", help="24 chapters, 2 saves, 1 restore round")
    mode.add_argument("--large-scenes", action="store_true",
                      help="300/1000 chapters, shallow and same-prose deep history, 3 measured runs")
    mode.add_argument("--sqlite-file-limit", action="store_true",
                      help="separate sparse 512 MiB + 1 byte input-size rejection control")
    parser.add_argument("--wall-seconds", type=int, default=MAX_WALL, help="reduce the time budget")
    parser.add_argument("--disk-mib", type=int, default=MAX_DISK // MIB, help="reduce the disk budget")
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("this supervisor requires Linux /proc and wait4 RSS semantics")
    if not 1 <= args.wall_seconds <= MAX_WALL or not 1 <= args.disk_mib <= MAX_DISK // MIB:
        parser.error("limits must be positive and cannot exceed 1800 s or 2048 MiB")
    binary = args.binary.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error("--binary must be a prebuilt executable")

    started = time.monotonic()
    deadline = started + args.wall_seconds
    directory_parent = REPO / ".superpowers" / "longform-scale-runs"
    if not directory_parent.resolve().is_relative_to(REPO):
        parser.error("run directory must stay inside the workspace")
    directory_parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="run-", dir=directory_parent))
    (directory / "staging").mkdir(mode=0o700)
    path = directory / "report.json"
    print(f"report: {path}", flush=True)
    disk_limit = args.disk_mib * MIB
    report = {
        "status": "incomplete",
        "mode": "large-scenes" if args.large_scenes else "sqlite-file-limit" if args.sqlite_file_limit else "smoke" if args.smoke else "full",
        "profile": args.profile,
        "utc_start": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "limits": {
            "wall_seconds": args.wall_seconds, "disk_bytes": disk_limit,
            "min_free_bytes": MIN_FREE, "stage_address_space_bytes": MAX_AS,
            "disk_guard_margin_bytes": min(64 * MIB, disk_limit // 20),
            "poll_seconds": POLL_SECONDS,
        },
        "measurement_notes": {
            "wall_seconds": "Stage wall time includes child launch and supervisor overhead.",
            "user_cpu_seconds/system_cpu_seconds": "Per-stage child CPU, including child-side launch.",
            "peak_rss_kib": (
                "Linux wait4 per-child lifetime high-water mark; may include inherited Python "
                "launcher pages before execve. Not a Rust-only peak or a cumulative parent metric. "
                "Excludes compilation and WebView."
            ),
            "sampled_peak_rss_kib": (
                "Largest observed /proc VmRSS sample, not an exact peak; 0 means no nonzero sample "
                "was observed, not proof of zero RSS."
            ),
            "metrics": "Narrower operation timings use Instant inside the Rust binary.",
        },
        "device": hardware(directory),
        "stages": [],
    }
    write_report(path, report)
    stages = [("seed", ["seed"])]
    if args.large_scenes:
        stages = [("large-seed", ["large-seed"])]
        for round_number in range(1, 4):
            for case in ("300-shallow", "1000-shallow", "1000-deep"):
                stages.append((f"{case}-{round_number}", ["large-measure", case, str(round_number)]))
    elif args.sqlite_file_limit:
        stages.extend([("grow-002", ["grow", "0", "2"]), ("sqlite-file-limit", ["sqlite-file-limit"])])
    else:
        previous = 0
        for n in ([1, 2] if args.smoke else [1, 32, 128]):
            stages.append((f"grow-{n:03}", ["grow", str(previous), str(n)]))
            stages.append((f"probe-{n:03}", ["probe", str(n)]))
            previous = n
        stages.append(("json", ["json", str(previous)]))
        for round_number in range(1, 2 if args.smoke else 4):
            stages.append((f"restore-{round_number}", ["restore", str(previous), str(round_number)]))

    for name, command in stages:
        print(f"starting {name}", flush=True)
        outcome = run_stage(binary, directory, name, command, deadline, disk_limit)
        report["stages"].append(outcome)
        report["elapsed_seconds"] = time.monotonic() - started
        write_report(path, report)
        if outcome["stop_reason"]:
            print(f"{name} incomplete: {outcome['stop_reason']}", file=sys.stderr, flush=True)
            return 1
        print(f"finished {name}: {outcome['wall_seconds']:.3f}s, "
              f"{outcome['peak_rss_kib']} KiB wait4 child lifetime peak RSS", flush=True)
    report["status"] = "complete"
    write_report(path, report)
    return 0


if __name__ == "__main__":
    sys.exit(main())
