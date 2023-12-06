
from updater.protocol import Usb


if __name__ == '__main__':
    gateway = Usb('/dev/ttyACM0', baudrate=115200)
    print(gateway.reader.serial.baudrate)
    assert gateway.is_open()

    gateway.transmit(bytearray([0, 1, 2, 255, 255, 5, 1, 2, 3, 254, 5, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5]))
    print(gateway.get_received(timeout=1))

    gateway.close()

