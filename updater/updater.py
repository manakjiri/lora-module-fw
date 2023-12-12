
from updater.protocol import Usb
from pathlib import Path
import argparse
from hashlib import sha256


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Update firmware')
    parser.add_argument('firmware', type=str, help='path to firmware binary')

    args = parser.parse_args()

    firmware_binary = Path(args.firmware).read_bytes()
    
    baudrate = 115200
    device_path = None
    for path in Path('/dev/').glob('ttyACM*'):
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
        raise RuntimeError('No gateway found')


    gateway = Usb(device_path, baudrate=baudrate)
    assert gateway.is_open()

    block_size = 32
    binary_size = len(firmware_binary)
    binary_sha = sha256(firmware_binary).digest()

    print('binary size:', binary_size, 'B')
    print('block size:', block_size, 'B')

    packet = bytearray([10])
    packet.extend(binary_size.to_bytes(4, 'little'))
    packet.extend(block_size.to_bytes(2, 'little'))
    packet.extend(binary_sha)
    gateway.transmit(packet)
    print(gateway.get_received(timeout=1))

    for i in range(0, binary_size, block_size):
        packet = bytearray([11])
        packet.extend((i // block_size).to_bytes(2, 'little')) # index
        end = i + block_size if i + block_size < binary_size else binary_size
        packet.extend(firmware_binary[i:end])
        gateway.transmit(packet)
        print(gateway.get_received(timeout=1))

    #gateway.transmit(bytearray([0, 1, 2, 255, 255, 5, 1, 2, 3, 254, 5, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5]))
    #print(gateway.get_received(timeout=1))

    gateway.close()

