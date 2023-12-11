
from updater.protocol import Usb
from pathlib import Path



if __name__ == '__main__':
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

    gateway.transmit(bytearray([0, 1, 2, 255, 255, 5, 1, 2, 3, 254, 5, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5]))
    print(gateway.get_received(timeout=1))

    gateway.close()

