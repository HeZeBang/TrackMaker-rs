#!/usr/bin/env python3
"""
自动化测试脚本 - 参数扫描和性能测试

该脚本通过修改 consts.rs 中的参数，自动运行 cargo build，
然后启动发送和接收进程进行测试。
"""

import subprocess
import os
import sys
import time
import json
import re
from pathlib import Path
from typing import List, Tuple, Dict, Any
from dataclasses import dataclass, asdict
import matplotlib.pyplot as plt
import numpy as np
from datetime import datetime


@dataclass
class TestResult:
    """单个测试结果"""
    config_name: str
    samples_per_level: int
    preamble_bytes: int
    difs_duration_ms: int
    cw_min: int
    cw_max: int
    slot_time_ms: int
    max_frame_data_size: int
    tx1_time: float
    tx2_time: float
    rx1_time: float
    rx2_time: float
    max_time: float
    repeat: int


class ExperimentConfig:
    """实验配置"""
    def __init__(self):
        self.repo_path = Path(__file__).parent.parent
        self.consts_file = self.repo_path / "src" / "utils" / "consts.rs"
        self.project_dir = self.repo_path
        self.log_dir = self.repo_path / "tmp" / "experiment_logs"
        self.log_dir.mkdir(parents=True, exist_ok=True)
        
    def get_parameter_combinations(self) -> List[Dict[str, Any]]:
        """
        定义参数组合列表
        修改这里来添加新的参数组合
        """
        return [
            {
                "name": "baseline",
                "SAMPLES_PER_LEVEL": 3,
                "PREAMBLE_PATTERN_BYTES": 4,
                "DIFS_DURATION_MS": 20,
                "CW_MIN": 10,
                "CW_MAX": 200,
                "SLOT_TIME_MS": 5,
                "MAX_FRAME_DATA_SIZE": 768,
            },
            {
                "name": "larger mincw",
                "SAMPLES_PER_LEVEL": 3,
                "PREAMBLE_PATTERN_BYTES": 4,
                "DIFS_DURATION_MS": 20,
                "CW_MIN": 50,
                "CW_MAX": 200,
                "SLOT_TIME_MS": 5,
                "MAX_FRAME_DATA_SIZE": 768,
            },
            {
                "name": "1024 frame",
                "SAMPLES_PER_LEVEL": 3,
                "PREAMBLE_PATTERN_BYTES": 4,
                "DIFS_DURATION_MS": 20,
                "CW_MIN": 10,
                "CW_MAX": 200,
                "SLOT_TIME_MS": 5,
                "MAX_FRAME_DATA_SIZE": 1024,
            },
            {
                "name": "512 frame",
                "SAMPLES_PER_LEVEL": 3,
                "PREAMBLE_PATTERN_BYTES": 4,
                "DIFS_DURATION_MS": 20,
                "CW_MIN": 10,
                "CW_MAX": 200,
                "SLOT_TIME_MS": 5,
                "MAX_FRAME_DATA_SIZE": 512,
            },
            {
                "name": "128 frame",
                "SAMPLES_PER_LEVEL": 3,
                "PREAMBLE_PATTERN_BYTES": 4,
                "DIFS_DURATION_MS": 20,
                "CW_MIN": 10,
                "CW_MAX": 200,
                "SLOT_TIME_MS": 5,
                "MAX_FRAME_DATA_SIZE": 128,
            },
            {
                "name": "robust_encoding",
                "SAMPLES_PER_LEVEL": 5,
                "PREAMBLE_PATTERN_BYTES": 8,
                "DIFS_DURATION_MS": 20,
                "CW_MIN": 10,
                "CW_MAX": 200,
                "SLOT_TIME_MS": 5,
                "MAX_FRAME_DATA_SIZE": 768,
            },
            {
                "name": "aggressive_backoff",
                "SAMPLES_PER_LEVEL": 3,
                "PREAMBLE_PATTERN_BYTES": 4,
                "DIFS_DURATION_MS": 20,
                "CW_MIN": 2,
                "CW_MAX": 32,
                "SLOT_TIME_MS": 2,
                "MAX_FRAME_DATA_SIZE": 768,
            },
            {
                "name": "conservative_backoff",
                "SAMPLES_PER_LEVEL": 3,
                "PREAMBLE_PATTERN_BYTES": 4,
                "DIFS_DURATION_MS": 30,
                "CW_MIN": 20,
                "CW_MAX": 400,
                "SLOT_TIME_MS": 8,
                "MAX_FRAME_DATA_SIZE": 768,
            },
            {
                "name": "short_difs",
                "SAMPLES_PER_LEVEL": 3,
                "PREAMBLE_PATTERN_BYTES": 4,
                "DIFS_DURATION_MS": 10,
                "CW_MIN": 10,
                "CW_MAX": 200,
                "SLOT_TIME_MS": 5,
                "MAX_FRAME_DATA_SIZE": 768,
            }
        ]


class ConstModifier:
    """修改 consts.rs 的工具类"""
    
    def __init__(self, consts_file: Path):
        self.consts_file = consts_file
        self.original_content = None
        
    def read_original(self):
        """读取原始文件内容"""
        self.original_content = self.consts_file.read_text()
        
    def restore(self):
        """恢复原始文件"""
        if self.original_content:
            self.consts_file.write_text(self.original_content)
            
    def update_params(self, params: Dict[str, Any]):
        """更新 consts.rs 中的参数"""
        content = self.consts_file.read_text()
        
        # 要更新的参数映射
        updates = {
            "SAMPLES_PER_LEVEL": params.get("SAMPLES_PER_LEVEL"),
            "PREAMBLE_PATTERN_BYTES": params.get("PREAMBLE_PATTERN_BYTES"),
            "DIFS_DURATION_MS": params.get("DIFS_DURATION_MS"),
            "CW_MIN": params.get("CW_MIN"),
            "CW_MAX": params.get("CW_MAX"),
            "SLOT_TIME_MS": params.get("SLOT_TIME_MS"),
            "MAX_FRAME_DATA_SIZE": params.get("MAX_FRAME_DATA_SIZE"),
        }
        
        for param_name, param_value in updates.items():
            if param_value is not None:
                # 匹配 pub const PARAM_NAME: ... = VALUE;
                pattern = rf"(pub const {param_name}: \w+\s*=\s*)\d+"
                replacement = rf"\g<1>{param_value}"
                content = re.sub(pattern, replacement, content)
        
        self.consts_file.write_text(content)


class ProcessManager:
    """管理子进程"""
    
    def __init__(self, project_dir: Path):
        self.project_dir = project_dir
        self.processes = {}
        self.start_times = {}
        self.end_times = {}
        
    def start_process(self, name: str, cmd: List[str], output_file: str) -> subprocess.Popen:
        """启动一个进程"""
        output_path = self.project_dir / "tmp" / output_file
        output_path.parent.mkdir(parents=True, exist_ok=True)
        
        with open(output_path, 'w') as f:
            proc = subprocess.Popen(
                cmd,
                stdout=f,
                stderr=subprocess.STDOUT,
                cwd=self.project_dir,
            )
        
        self.processes[name] = proc
        self.start_times[name] = time.time()
        print(f"  ✓ Started {name} (PID: {proc.pid})")
        return proc
        
    def wait_for_process(self, name: str, timeout: float = None) -> bool:
        """等待指定进程完成"""
        if name not in self.processes:
            return False
            
        try:
            self.processes[name].wait(timeout=timeout)
            self.end_times[name] = time.time()
            return True
        except subprocess.TimeoutExpired:
            return False
            
    def get_duration(self, name: str) -> float:
        """获取进程运行时间"""
        if name in self.start_times and name in self.end_times:
            return self.end_times[name] - self.start_times[name]
        return -1
        
    def terminate_all(self):
        """终止所有进程"""
        for name, proc in self.processes.items():
            if proc.poll() is None:  # 如果进程还在运行
                print(f"  ✗ Terminating {name} (PID: {proc.pid})")
                proc.terminate()
                try:
                    proc.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    
    def clear(self):
        """清空进程列表"""
        self.processes.clear()
        self.start_times.clear()
        self.end_times.clear()


class ExperimentRunner:
    """实验运行器"""
    
    def __init__(self, config: ExperimentConfig):
        self.config = config
        self.modifier = ConstModifier(config.consts_file)
        self.results: List[TestResult] = []
        
    def build(self) -> bool:
        """构建项目"""
        print("  Building project...")
        result = subprocess.run(
            ["cargo", "build"],
            cwd=self.config.project_dir,
            capture_output=True,
            timeout=120,
        )
        return result.returncode == 0
        
    def run_single_test(
        self,
        param_config: Dict[str, Any],
        repeat_idx: int,
    ) -> TestResult:
        """运行单个测试"""
        config_name = param_config["name"]
        print(f"\n📊 Test Config: {config_name} (Repeat {repeat_idx + 1}/4)")
        print(f"  Parameters:")
        print(f"    - SAMPLES_PER_LEVEL: {param_config['SAMPLES_PER_LEVEL']}")
        print(f"    - PREAMBLE_PATTERN_BYTES: {param_config['PREAMBLE_PATTERN_BYTES']}")
        print(f"    - DIFS_DURATION_MS: {param_config['DIFS_DURATION_MS']}")
        print(f"    - CW_MIN: {param_config['CW_MIN']}")
        print(f"    - CW_MAX: {param_config['CW_MAX']}")
        print(f"    - SLOT_TIME_MS: {param_config['SLOT_TIME_MS']}")
        print(f"    - MAX_FRAME_DATA_SIZE: {param_config['MAX_FRAME_DATA_SIZE']}")
        
        # 1. 更新参数
        print("  Updating consts.rs...")
        self.modifier.update_params(param_config)
        
        # 2. 编译
        print("  Building...")
        if not self.build():
            print("  ❌ Build failed!")
            return None
            
        # 3. 清理之前的输出文件
        for f in ["tx1.log", "tx2.log", "rx1.log", "rx2.log"]:
            log_file = self.config.project_dir / "tmp" / f
            if log_file.exists():
                log_file.unlink()
        
        # 4. 启动进程
        print("  Starting processes...")
        pm = ProcessManager(self.config.project_dir)
        
        # 启动两个发送端和两个接收端
        pm.start_process("rx2", ["cargo", "run", "--release", "--", "rx", "-l", "2", "-r", "1", "-d", "40"], "rx2.log")
        time.sleep(1)
        pm.start_process("rx1", ["cargo", "run", "--release", "--", "rx", "-l", "1", "-r", "2", "-d", "40"], "rx1.log")
        time.sleep(1)
        pm.start_process("tx2", ["cargo", "run", "--release", "--", "tx", "-l", "2", "-r", "1"], "tx2.log")
        time.sleep(0.5)
        pm.start_process("tx1", ["cargo", "run", "--release", "--", "tx", "-l", "1", "-r", "2"], "tx1.log")
        
        # 5. 等待进程完成
        print("  Waiting for processes to complete...")
        tx_processes = {"tx1", "tx2"}
        rx_processes = {"rx1", "rx2"}
        tx_completed = set()
        rx_completed = set()
        
        # 设置总超时时间（应该足够完成测试）
        total_timeout = 120
        start_wait = time.time()
        
        while time.time() - start_wait < total_timeout:
            # 检查 TX 进程
            for tx_name in tx_processes - tx_completed:
                if pm.processes[tx_name].poll() is not None:
                    pm.end_times[tx_name] = time.time()
                    tx_completed.add(tx_name)
                    print(f"    ✓ {tx_name} completed ({pm.get_duration(tx_name):.2f}s)")
            
            # 检查 RX 进程
            for rx_name in rx_processes - rx_completed:
                if pm.processes[rx_name].poll() is not None:
                    pm.end_times[rx_name] = time.time()
                    rx_completed.add(rx_name)
                    print(f"    ✓ {rx_name} completed ({pm.get_duration(rx_name):.2f}s)")
            
            # 当所有 TX 和所有 RX 都完成时，退出等待
            if tx_completed == tx_processes or rx_completed == rx_processes:
                print("  Transmission completed / timeout!")
                break
                
            time.sleep(0.5)
        
        # 6. 终止所有未完成的进程
        if tx_completed != tx_processes or rx_completed != rx_processes:
            print("  Terminating remaining processes...")
            pm.terminate_all()
        
        # 7. 收集结果
        tx1_time = pm.get_duration("tx1")
        tx2_time = pm.get_duration("tx2")
        rx1_time = pm.get_duration("rx1")
        rx2_time = pm.get_duration("rx2")
        
        print(f"  Results:")
        print(f"    - TX1 time: {tx1_time:.2f}s")
        print(f"    - TX2 time: {tx2_time:.2f}s")
        print(f"    - RX1 time: {rx1_time:.2f}s")
        print(f"    - RX2 time: {rx2_time:.2f}s")
        
        max_time = max(tx1_time, tx2_time, rx1_time, rx2_time)
        
        result = TestResult(
            config_name=config_name,
            samples_per_level=param_config["SAMPLES_PER_LEVEL"],
            preamble_bytes=param_config["PREAMBLE_PATTERN_BYTES"],
            difs_duration_ms=param_config["DIFS_DURATION_MS"],
            cw_min=param_config["CW_MIN"],
            cw_max=param_config["CW_MAX"],
            slot_time_ms=param_config["SLOT_TIME_MS"],
            max_frame_data_size=param_config["MAX_FRAME_DATA_SIZE"],
            tx1_time=tx1_time,
            tx2_time=tx2_time,
            rx1_time=rx1_time,
            rx2_time=rx2_time,
            max_time=max_time,
            repeat=repeat_idx + 1,
        )
        
        return result
        
    def run_all_experiments(self):
        """运行所有实验"""
        print("=" * 80)
        print("🚀 Starting Automated Testing")
        print("=" * 80)
        
        # 保存原始文件
        self.modifier.read_original()
        
        param_combinations = self.config.get_parameter_combinations()
        
        try:
            # 对每个参数组合进行4次重复测试
            for param_config in param_combinations:
                for repeat_idx in range(4):
                    result = self.run_single_test(param_config, repeat_idx)
                    if result:
                        self.results.append(result)
                    time.sleep(2)  # 测试之间的间隔
        finally:
            # 恢复原始文件
            print("\n  Restoring consts.rs...")
            self.modifier.restore()
        
        # 保存结果
        self.save_results()
        
        # 绘制图表
        self.plot_results()
        
        print("\n" + "=" * 80)
        print("✅ All experiments completed!")
        print("=" * 80)
        
    def save_results(self):
        """保存结果到 JSON 文件"""
        results_file = self.config.log_dir / f"results_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json"
        
        data = {
            "timestamp": datetime.now().isoformat(),
            "results": [asdict(r) for r in self.results],
        }
        
        with open(results_file, 'w') as f:
            json.dump(data, f, indent=2)
        
        print(f"\n📝 Results saved to: {results_file}")
        
    def plot_results(self):
        """绘制结果图表"""
        if not self.results:
            print("No results to plot")
            return
        
        # 按 config_name 分组
        grouped = {}
        for result in self.results:
            if result.config_name not in grouped:
                grouped[result.config_name] = []
            grouped[result.config_name].append(result)
        
        config_names = list(grouped.keys())
        max_times = []
        max_times_std = []
        
        for config_name in config_names:
            times = [r.max_time for r in grouped[config_name]]
            max_times.append(np.mean(times))
            max_times_std.append(np.std(times))
        
        # 绘制柱状图
        fig, axes = plt.subplots(2, 3, figsize=(18, 10))
        fig.suptitle('Performance Analysis by Configuration', fontsize=16, fontweight='bold')
        
        # 1. 最大运行时间对比
        ax1 = axes[0, 0]
        x_pos = np.arange(len(config_names))
        ax1.bar(x_pos, max_times, yerr=max_times_std, capsize=5, alpha=0.7, color='steelblue')
        ax1.set_xlabel('Configuration')
        ax1.set_ylabel('Time (seconds)')
        ax1.set_title('Max Process Completion Time (Mean ± Std)')
        ax1.set_xticks(x_pos)
        ax1.set_xticklabels(config_names, rotation=45, ha='right')
        ax1.grid(axis='y', alpha=0.3)
        
        # 2. 详细时间对比
        ax2 = axes[0, 1]
        width = 0.2
        for i, config_name in enumerate(config_names):
            times = [r.max_time for r in grouped[config_name]]
            x_offset = x_pos[i] + (width * (len(config_names) - 1) / 2)
            ax2.scatter([i] * len(times), times, alpha=0.6, s=50, label=config_name if i == 0 else "")
        
        ax2.set_xlabel('Configuration')
        ax2.set_ylabel('Time (seconds)')
        ax2.set_title('Individual Run Times by Configuration')
        ax2.set_xticks(x_pos)
        ax2.set_xticklabels(config_names, rotation=45, ha='right')
        ax2.grid(alpha=0.3)
        
        # 3. 参数影响分析：SAMPLES_PER_LEVEL vs 运行时间
        ax3 = axes[0, 2]
        samples_levels = []
        times_by_sample = []
        for config_name in config_names:
            samples = grouped[config_name][0].samples_per_level
            times = [r.max_time for r in grouped[config_name]]
            samples_levels.append(samples)
            times_by_sample.append(np.mean(times))
        
        ax3.plot(samples_levels, times_by_sample, marker='o', linewidth=2, markersize=8, color='green')
        ax3.set_xlabel('SAMPLES_PER_LEVEL')
        ax3.set_ylabel('Average Max Time (seconds)')
        ax3.set_title('Impact of SAMPLES_PER_LEVEL')
        ax3.grid(alpha=0.3)
        
        # 4. 参数影响分析：CW_MIN vs 运行时间
        ax4 = axes[1, 0]
        cw_mins = []
        times_by_cw = []
        for config_name in config_names:
            cw_min = grouped[config_name][0].cw_min
            times = [r.max_time for r in grouped[config_name]]
            cw_mins.append(cw_min)
            times_by_cw.append(np.mean(times))
        
        ax4.plot(cw_mins, times_by_cw, marker='s', linewidth=2, markersize=8, color='orange')
        ax4.set_xlabel('CW_MIN')
        ax4.set_ylabel('Average Max Time (seconds)')
        ax4.set_title('Impact of CW_MIN (Backoff Window)')
        ax4.grid(alpha=0.3)
        
        # 5. 参数影响分析：MAX_FRAME_DATA_SIZE vs 运行时间
        ax5 = axes[1, 1]
        frame_sizes = []
        times_by_frame = []
        for config_name in config_names:
            frame_size = grouped[config_name][0].max_frame_data_size
            times = [r.max_time for r in grouped[config_name]]
            frame_sizes.append(frame_size)
            times_by_frame.append(np.mean(times))
        
        ax5.plot(frame_sizes, times_by_frame, marker='^', linewidth=2, markersize=8, color='red')
        ax5.set_xlabel('MAX_FRAME_DATA_SIZE (bytes)')
        ax5.set_ylabel('Average Max Time (seconds)')
        ax5.set_title('Impact of MAX_FRAME_DATA_SIZE')
        ax5.grid(alpha=0.3)
        
        # 6. 参数影响分析：DIFS_DURATION_MS vs 运行时间
        ax6 = axes[1, 2]
        difs_durations = []
        times_by_difs = []
        for config_name in config_names:
            difs_duration = grouped[config_name][0].difs_duration_ms
            times = [r.max_time for r in grouped[config_name]]
            difs_durations.append(difs_duration)
            times_by_difs.append(np.mean(times))
        
        ax6.plot(difs_durations, times_by_difs, marker='D', linewidth=2, markersize=8, color='purple')
        ax6.set_xlabel('DIFS_DURATION_MS (milliseconds)')
        ax6.set_ylabel('Average Max Time (seconds)')
        ax6.set_title('Impact of DIFS_DURATION_MS')
        ax6.grid(alpha=0.3)
        
        plt.tight_layout()
        
        # 保存图表
        plot_file = self.config.log_dir / f"results_{datetime.now().strftime('%Y%m%d_%H%M%S')}.png"
        plt.savefig(plot_file, dpi=150, bbox_inches='tight')
        print(f"📊 Plot saved to: {plot_file}")
        
        # 显示图表
        plt.show()


def main():
    """主函数"""
    try:
        config = ExperimentConfig()
        runner = ExperimentRunner(config)
        runner.run_all_experiments()
    except KeyboardInterrupt:
        print("\n❌ Experiment interrupted by user")
        sys.exit(1)
    except Exception as e:
        print(f"\n❌ Error: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    main()
