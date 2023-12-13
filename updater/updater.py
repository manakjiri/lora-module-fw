from updater.protocol import Usb
from pathlib import Path
import argparse
from hashlib import sha256
import time


def init_packet(binary: bytes, block_size=32) -> bytes:
    binary_size = len(binary)
    binary_sha = sha256(binary).digest()
    packet = bytearray([10])
    packet.extend(binary_size.to_bytes(4, "little"))
    packet.extend(block_size.to_bytes(2, "little"))
    packet.extend(binary_sha)
    return bytes(packet)


def main():
    parser = argparse.ArgumentParser(description="Update firmware")
    parser.add_argument("firmware", type=str, help="path to firmware binary")

    args = parser.parse_args()
    firmware_binary = Path(args.firmware).read_bytes()
    baudrate = 115200
    block_size = 32
    binary_size = len(firmware_binary)

    device_path = None
    for path in Path("/dev/").glob("ttyACM*"):
        try:
            gateway = Usb(str(path), baudrate=baudrate)
            data = bytearray([0, 1, 2, 255, 255, 5, 1])
            gateway.transmit(data)
            assert gateway.get_received(timeout=0.1) == data
            gateway.close()
            device_path = str(path)
            break
        except:
            pass
    else:
        raise RuntimeError("No gateway found")

    gateway = Usb(device_path, baudrate=baudrate)
    assert gateway.is_open()

    print("binary size:", len(firmware_binary), "B")
    print("block size:", block_size, "B")

    gateway.transmit(init_packet(firmware_binary, block_size))
    resp = gateway.get_received(timeout=1)
    if resp is None:
        print("cancelling ongoing update")
        gateway.transmit(bytearray([12]))

        resp = gateway.get_received(timeout=1)
        if resp is None:
            raise RuntimeError("No response from gateway")

        # retry
        time.sleep(0.5)
        gateway.transmit(init_packet(firmware_binary, block_size))
        resp = gateway.get_received(timeout=1)
        if resp is None:
            raise RuntimeError("No response from gateway")

    time.sleep(0.5)

    # add all indexes initially
    indexes = [i for i in range(0, binary_size, block_size)]

    for i in indexes:
        print("transmitting block:", i // block_size, "of", binary_size // block_size)
        packet = bytearray([11])
        packet.extend((i // block_size).to_bytes(2, "little"))  # index
        end = i + block_size if i + block_size < binary_size else binary_size
        packet.extend(firmware_binary[i:end])
        gateway.transmit(packet)
        resp = gateway.get_received(timeout=1)
        if resp and len(resp) > 1:
            for index in resp[1:]:
                print("failed block:", index, "retrying")
                indexes.append(index * block_size)

        time.sleep(0.5)

    # gateway.transmit(bytearray([0, 1, 2, 255, 255, 5, 1, 2, 3, 254, 5, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5]))
    # print(gateway.get_received(timeout=1))

    gateway.close()


if __name__ == "__main__":
    main()
