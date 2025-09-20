"""
Audio Data Visualization Tool
从JSON文件加载音频数据并生成多种可视化图表
"""

import json
import numpy as np
import pandas as pd
import plotly.graph_objects as go
import plotly.express as px
from plotly.subplots import make_subplots
from scipy import signal
import os

def load_audio_data(json_file_path):
    """从JSON文件加载音频数据"""
    try:
        with open(json_file_path, 'r') as f:
            data = json.load(f)
        print(f"成功加载JSON文件: {json_file_path}")
        return data
    except FileNotFoundError:
        print(f"文件未找到: {json_file_path}")
        return None
    except json.JSONDecodeError:
        print(f"JSON解析错误: {json_file_path}")
        return None

def create_sample_data():
    """创建示例音频数据（如果JSON文件不存在或格式不正确）"""
    sample_rate = 44100
    duration = 2  # 2秒
    t = np.linspace(0, duration, int(sample_rate * duration))
    
    # 生成复合信号：基频 + 谐波 + 噪声
    fundamental = 440  # A4音符
    signal_data = (
        0.5 * np.sin(2 * np.pi * fundamental * t) +
        0.3 * np.sin(2 * np.pi * fundamental * 2 * t) +
        0.1 * np.sin(2 * np.pi * fundamental * 3 * t) +
        0.05 * np.random.normal(0, 1, len(t))
    )
    
    return {
        'sample_rate': sample_rate,
        'audio_data': signal_data.tolist(),
        'duration': duration,
        'channels': 1
    }

def plot_waveform(audio_data, sample_rate):
    """绘制音频波形图"""
    time = np.linspace(0, len(audio_data) / sample_rate, len(audio_data))
    
    fig = go.Figure()
    fig.add_trace(go.Scatter(
        x=time,
        y=audio_data,
        mode='lines',
        name='音频波形',
        line=dict(color='blue', width=1)
    ))
    
    fig.update_layout(
        title='音频波形图',
        xaxis_title='时间 (秒)',
        yaxis_title='振幅',
        template='plotly_white',
        hovermode='x unified'
    )
    
    return fig

def plot_spectrum(audio_data, sample_rate):
    """绘制频谱图"""
    # 计算FFT
    fft = np.fft.fft(audio_data)
    freqs = np.fft.fftfreq(len(audio_data), 1/sample_rate)
    
    # 只取正频率部分
    positive_freqs = freqs[:len(freqs)//2]
    magnitude = np.abs(fft[:len(fft)//2])
    
    fig = go.Figure()
    fig.add_trace(go.Scatter(
        x=positive_freqs,
        y=20 * np.log10(magnitude + 1e-10),  # 转换为dB
        mode='lines',
        name='频谱',
        line=dict(color='red', width=1)
    ))
    
    fig.update_layout(
        title='频谱图',
        xaxis_title='频率 (Hz)',
        yaxis_title='幅度 (dB)',
        template='plotly_white',
        xaxis=dict(range=[0, sample_rate//2])
    )
    
    return fig

def plot_spectrogram(audio_data, sample_rate):
    """绘制频谱图（时频图）"""
    # 计算短时傅里叶变换
    frequencies, times, Sxx = signal.spectrogram(
        audio_data, 
        sample_rate, 
        nperseg=1024,
        noverlap=512
    )
    
    # 转换为dB
    Sxx_db = 10 * np.log10(Sxx + 1e-10)
    
    fig = go.Figure(data=go.Heatmap(
        z=Sxx_db,
        x=times,
        y=frequencies,
        colorscale='Viridis',
        colorbar=dict(title='幅度 (dB)')
    ))
    
    fig.update_layout(
        title='频谱图 (时频分析)',
        xaxis_title='时间 (秒)',
        yaxis_title='频率 (Hz)',
        template='plotly_white'
    )
    
    return fig

def create_audio_dashboard(audio_data, sample_rate):
    """创建音频分析仪表板"""
    # 创建子图
    fig = make_subplots(
        rows=2, cols=2,
        subplot_titles=('波形图', '频谱图', '频谱图', '统计信息'),
        specs=[[{"secondary_y": False}, {"secondary_y": False}],
               [{"type": "heatmap"}, {"type": "table"}]]
    )
    
    # 波形图
    time = np.linspace(0, len(audio_data) / sample_rate, len(audio_data))
    fig.add_trace(
        go.Scatter(x=time, y=audio_data, mode='lines', name='波形'),
        row=1, col=1
    )
    
    # 频谱图
    fft = np.fft.fft(audio_data)
    freqs = np.fft.fftfreq(len(audio_data), 1/sample_rate)
    positive_freqs = freqs[:len(freqs)//2]
    magnitude = np.abs(fft[:len(fft)//2])
    
    fig.add_trace(
        go.Scatter(x=positive_freqs, y=20 * np.log10(magnitude + 1e-10), 
                  mode='lines', name='频谱'),
        row=1, col=2
    )
    
    # 频谱图
    frequencies, times, Sxx = signal.spectrogram(
        audio_data, sample_rate, nperseg=1024, noverlap=512
    )
    Sxx_db = 10 * np.log10(Sxx + 1e-10)
    
    fig.add_trace(
        go.Heatmap(z=Sxx_db, x=times, y=frequencies, 
                  colorscale='Viridis', showscale=False),
        row=2, col=1
    )
    
    # 统计信息表格
    stats = {
        '统计项': ['最大值', '最小值', '均值', '标准差', '峰值因子', 'RMS'],
        '数值': [
            f"{np.max(audio_data):.4f}",
            f"{np.min(audio_data):.4f}",
            f"{np.mean(audio_data):.4f}",
            f"{np.std(audio_data):.4f}",
            f"{np.max(np.abs(audio_data)) / (np.sqrt(np.mean(audio_data**2)) + 1e-10):.4f}",
            f"{np.sqrt(np.mean(audio_data**2)):.4f}"
        ]
    }
    
    fig.add_trace(
        go.Table(
            header=dict(values=list(stats.keys())),
            cells=dict(values=list(stats.values()))
        ),
        row=2, col=2
    )
    
    fig.update_layout(
        height=800,
        title_text="音频分析仪表板",
        showlegend=False
    )
    
    return fig

def plot_3d_visualization(audio_data, sample_rate):
    """创建3D音频可视化"""
    # 计算短时傅里叶变换
    frequencies, times, Sxx = signal.spectrogram(
        audio_data, sample_rate, nperseg=512, noverlap=256
    )
    
    # 创建3D网格
    T, F = np.meshgrid(times, frequencies)
    Z = 10 * np.log10(Sxx + 1e-10)
    
    fig = go.Figure(data=[go.Surface(
        x=T, y=F, z=Z,
        colorscale='Viridis',
        colorbar=dict(title='幅度 (dB)')
    )])
    
    fig.update_layout(
        title='3D 时频分析',
        scene=dict(
            xaxis_title='时间 (秒)',
            yaxis_title='频率 (Hz)',
            zaxis_title='幅度 (dB)'
        ),
        template='plotly_white'
    )
    
    return fig

def main():
    """主函数"""
    # JSON文件路径
    json_file = '../tmp/output.json'
    
    print("=== 音频数据可视化工具 ===")
    
    # 尝试加载JSON数据
    data = load_audio_data(json_file)
    
    if data is None:
        print("使用示例数据进行演示...")
        data = create_sample_data()
    
    # 提取音频数据
    try:
        audio_data = np.array(data.get('audio_data', []))
        sample_rate = data.get('sample_rate', 48000)
        
        if len(audio_data) == 0:
            raise ValueError("音频数据为空")
            
        print(f"音频数据长度: {len(audio_data)}")
        print(f"采样率: {sample_rate} Hz")
        print(f"持续时间: {len(audio_data)/sample_rate:.2f} 秒")
        
    except (KeyError, ValueError) as e:
        print(f"数据格式错误: {e}")
        print("使用示例数据...")
        data = create_sample_data()
        audio_data = np.array(data['audio_data'])
        sample_rate = data['sample_rate']
    
    # 生成各种可视化图表
    print("\n正在生成可视化图表...")
    
    # 1. 波形图
    print("1. 生成波形图...")
    waveform_fig = plot_waveform(audio_data, sample_rate)
    waveform_fig.show()
    
    # 2. 频谱图
    print("2. 生成频谱图...")
    spectrum_fig = plot_spectrum(audio_data, sample_rate)
    spectrum_fig.show()
    
    # 3. 频谱图（时频分析）
    print("3. 生成频谱图...")
    spectrogram_fig = plot_spectrogram(audio_data, sample_rate)
    spectrogram_fig.show()
    
    # 4. 仪表板
    print("4. 生成分析仪表板...")
    dashboard_fig = create_audio_dashboard(audio_data, sample_rate)
    dashboard_fig.show()
    
    # 5. 3D可视化
    print("5. 生成3D可视化...")
    viz_3d_fig = plot_3d_visualization(audio_data, sample_rate)
    viz_3d_fig.show()
    
    print("\n所有可视化图表已生成完成！")
    print("图表将在浏览器中打开。")

if __name__ == "__main__":
    main()