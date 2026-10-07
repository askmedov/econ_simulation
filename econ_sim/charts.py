"""Overview chart of runs, with and without forced events (needs matplotlib)."""

from __future__ import annotations

from pathlib import Path

import numpy as np

from econ_sim.config import ScheduledEvent
from econ_sim.scenarios import series, spread
from econ_sim.simulation import Simulation

SURFACE = "#fcfcfb"
TEXT = "#0b0b0b"
TEXT_SECONDARY = "#52514e"
MUTED = "#898781"
GRID = "#e1e0d9"
BASELINE_COLOR = "#2a78d6"  # categorical slot 1 (blue)
SCENARIO_COLOR = "#eb6834"  # categorical slot 2 (orange)

# (metric, panel title, cumulative?)
PANELS = (
    ("food_stock", "Food in store (rations)", False),
    ("ration", "Share of food need met (%)", False),
    ("avg_health", "Average health (0-100)", False),
    ("deaths", "Deaths so far", True),
)


def event_label(forced: tuple[ScheduledEvent, ...]) -> str:
    return ", ".join(f"{e.event.replace('_', ' ')} in month {e.month}" for e in forced)


def plot(
    path: Path,
    baseline: list[Simulation],
    scenario: list[Simulation] | None = None,
    forced: tuple[ScheduledEvent, ...] = (),
) -> None:
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from matplotlib.ticker import FuncFormatter, MaxNLocator

    months = np.array([r.month_number for r in baseline[0].records])
    lines = [(baseline, BASELINE_COLOR, "Without the event" if scenario else "Village")]
    if scenario is not None:
        lines.append((scenario, SCENARIO_COLOR, f"With {event_label(forced)}"))

    fig, axes = plt.subplots(2, 2, figsize=(11, 6.6), sharex=True, facecolor=SURFACE)
    for ax, (metric, title, cumulative) in zip(axes.flat, PANELS):
        ax.set_facecolor(SURFACE)
        ax.set_title(title, loc="left", fontsize=11, color=TEXT, pad=8)
        ax.grid(axis="y", color=GRID, linewidth=0.8)
        ax.set_axisbelow(True)
        for side in ("top", "right", "left"):
            ax.spines[side].set_visible(False)
        ax.spines["bottom"].set_color(GRID)
        ax.tick_params(colors=MUTED, labelsize=9, length=0)
        ax.yaxis.set_major_locator(MaxNLocator(nbins=4, integer=True))
        ax.yaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:,.0f}"))

        ends = []
        for sims, color, label in lines:
            values = series(sims, metric)
            if cumulative:
                values = values.cumsum(axis=1)
            if metric == "ration":
                values = values * 100
            s = spread(values)
            if len(sims) > 1:
                ax.fill_between(months, s.low, s.high, color=color, alpha=0.14, linewidth=0)
            ax.plot(months, s.mean, color=color, linewidth=1.6, solid_capstyle="round", label=label)
            ends.append(s.mean[-1])

        for item in forced:
            ax.axvline(item.month, color=MUTED, linewidth=0.8)
        ax.set_ylim(bottom=0)
        if metric == "ration":
            ax.set_ylim(0, 105)
        if metric == "avg_health":
            ax.set_ylim(0, 100)
        ax.set_xlim(months[0], months[-1] + max(1, len(months) // 12))
        _label_ends(ax, months[-1], ends, TEXT_SECONDARY)

    for ax in axes[1]:
        ax.set_xlabel("Month", color=MUTED, fontsize=9)
        ax.xaxis.set_major_locator(MaxNLocator(nbins=8, integer=True))
    for item in forced:
        axes[0, 0].annotate(
            item.event.replace("_", " "),
            xy=(item.month, 1.0),
            xycoords=("data", "axes fraction"),
            xytext=(3, -2),
            textcoords="offset points",
            fontsize=8,
            color=TEXT_SECONDARY,
            va="top",
        )

    runs = len(baseline)
    subtitle = "One run" if runs == 1 else f"Average of {runs} runs; shading shows where 80% of runs fall"
    fig.suptitle(f"Village over {len(months)} months", x=0.06, y=0.98, ha="left", va="top", fontsize=13, color=TEXT)
    fig.text(0.06, 0.935, subtitle, fontsize=9, color=TEXT_SECONDARY, va="top")
    if scenario is not None:
        fig.legend(
            *axes[0, 0].get_legend_handles_labels(),
            loc="upper right",
            bbox_to_anchor=(0.98, 0.99),
            ncol=2,
            frameon=False,
            fontsize=9,
            labelcolor=TEXT_SECONDARY,
        )
    fig.tight_layout(rect=(0, 0, 1, 0.94))
    path.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(path, dpi=150, facecolor=SURFACE)
    plt.close(fig)


def _label_ends(ax, x: float, values: list[float], color: str) -> None:
    """Value labels at the line ends; skip one if two would overlap."""
    low, high = ax.get_ylim()
    placed: list[float] = []
    for value in values:
        if any(abs(value - other) < 0.06 * (high - low) for other in placed):
            continue
        ax.annotate(
            f"{value:,.0f}",
            xy=(x, value),
            xytext=(4, 0),
            textcoords="offset points",
            fontsize=8,
            color=color,
            va="center",
        )
        placed.append(value)
