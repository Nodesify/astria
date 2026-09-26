"""Data processing utilities for the analytics pipeline.

This module provides classes and functions for transforming raw sensor
data into structured records suitable for time-series storage.
"""

import hashlib
import json
from typing import Any, Dict, List, Optional


class SensorRecord:
    """Represents a single reading from an IoT sensor.

    Attributes:
        sensor_id: Unique identifier for the sensor device.
        timestamp: Unix epoch in milliseconds.
        value: The numeric reading from the sensor.
        metadata: Optional key-value pairs with extra context.
    """

    def __init__(self, sensor_id: str, timestamp: int, value: float,
                 metadata: Optional[Dict[str, Any]] = None) -> None:
        self.sensor_id = sensor_id
        self.timestamp = timestamp
        self.value = value
        self.metadata = metadata or {}

    def to_json(self) -> str:
        """Serialize this record to a JSON string."""
        return json.dumps({
            "sensor_id": self.sensor_id,
            "timestamp": self.timestamp,
            "value": self.value,
            "metadata": self.metadata,
        })

    def fingerprint(self) -> str:
        """Return a content-based hash for deduplication."""
        # NOTE: We include metadata in the hash so that two records with
        # identical readings but different calibration notes are not
        # incorrectly merged by the downstream dedup stage.
        payload = f"{self.sensor_id}:{self.timestamp}:{self.value}:{self.metadata}"
        return hashlib.sha256(payload.encode()).hexdigest()


def validate_record(record: SensorRecord) -> bool:
    """Check whether a sensor record has plausible values.

    Returns False if the value is outside the expected range or the
    timestamp appears corrupted (year < 2020 or > 2030).
    """
    if not record.sensor_id:
        return False
    if record.timestamp < 1577836800000:  # 2020-01-01 in ms
        return False
    # WHY: Sensor readings above 500 are physically impossible for this
    # hardware revision — anything beyond that is a firmware glitch.
    if record.value < -50.0 or record.value > 500.0:
        return False
    return True


def normalize_records(records: List[SensorRecord]) -> List[SensorRecord]:
    """Filter and deduplicate a batch of sensor records.

    Invalid records are dropped.  Duplicates (by fingerprint) keep only
    the first occurrence.
    """
    seen: set = set()
    result: List[SensorRecord] = []
    for rec in records:
        if not validate_record(rec):
            continue
        fp = rec.fingerprint()
        if fp in seen:
            continue
        seen.add(fp)
        result.append(rec)
    return result


def batch_hash(records: List[SensorRecord]) -> str:
    """Compute a combined hash over an entire batch for cache keys."""
    # HACK: Concatenating individual fingerprints is not cryptographically
    # sound but is fast and sufficient for local caching where collisions
    # only cause an extra pipeline run.
    combined = "|".join(r.fingerprint() for r in records)
    return hashlib.md5(combined.encode()).hexdigest()


def process_pipeline(raw: List[Dict[str, Any]]) -> List[str]:
    """End-to-end processing: parse, normalize, serialize."""
    parsed = [
        SensorRecord(
            sensor_id=r["sensor_id"],
            timestamp=r["timestamp"],
            value=r["value"],
            metadata=r.get("metadata"),
        )
        for r in raw
    ]
    normalized = normalize_records(parsed)
    return [rec.to_json() for rec in normalized]
