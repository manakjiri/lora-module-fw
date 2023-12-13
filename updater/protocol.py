from crc import Calculator, Crc32
from serial import Serial
from serial.threaded import ReaderThread, Protocol
from queue import Queue, Empty
from time import ctime, time
from contextlib import contextmanager
from typing import Type


class Interface:
    def __init__(self, serial_port, protocol_reader) -> None:
        self.rx_queue = Queue()
        # Initiate ReaderThread
        protocol_reader.callback = self._callback
        self.reader = ReaderThread(serial_port, protocol_reader)
        # Start reader
        self.reader.start()

    def _callback(self, p):
        self.rx_queue.put(p, timeout=0.1)

    def get_received(self, timeout=0.0) -> bytearray | None:
        try:
            if not self.rx_queue.empty() or timeout:
                ret = self.rx_queue.get(timeout=timeout)
                # print(ctime(), "RX:", ret.hex(" "))
                return ret
        except Empty:
            return None

    def get_all_received(self, timeout=0.0) -> list[bytearray]:
        ret = []
        start = time()
        while not timeout and not self.rx_queue.empty():
            p = self.get_received(timeout)
            if p:
                ret.append(p)

            if (not p) or (timeout and start + timeout > time()):
                break

        return ret

    def transmit(self, p: bytearray):
        raise NotImplementedError(
            "Interface base class does not implement transmit(), use one of the derived classes"
        )

    def close(self):
        self.reader.close()
        self.reader.join()

    def is_open(self):
        return self.reader.serial.is_open

    def port_path(self):
        return self.reader.serial.port


def maxval_encode(data: bytearray, max_val: int) -> bytearray:
    out = bytearray()
    for d in data:
        if d >= max_val:
            out.extend([max_val, d - max_val])
        else:
            out.append(d)
    return out


def maxval_decode(data: bytearray, max_val: int) -> bytearray:
    out = bytearray()
    next_add = False
    for d in data:
        if d == max_val:
            next_add = True
            continue
        out.append(d + max_val if next_add else d)
        next_add = False
    return out


class UsbReaderProtocol(Protocol):
    callback = None
    rx_buffer = None

    def connection_made(self, transport):
        """Called when reader thread is started"""
        # print("Connected")
        self.rx_buffer = bytearray()

    def data_received(self, data):
        """Called with snippets received from the serial port"""
        # print(ctime(), "RX:", data.hex(" "))

        try:
            self.rx_buffer.extend(data)

            i = self.rx_buffer.index(b"\xFF")
            if i > 0:
                if self.callback:
                    data = maxval_decode(self.rx_buffer[:i], 254)
                    self.callback(data)

                self.rx_buffer = self.rx_buffer[i + 1 :]

        except Exception as e:
            # print(e)
            pass


class Usb(Interface):
    def __init__(self, port, **kwargs) -> None:
        super().__init__(Serial(port, **kwargs), UsbReaderProtocol)

    def transmit(self, p: bytearray):
        # print(ctime(), "TX:", p.hex(" "))
        encoded = maxval_encode(p, 254)
        encoded.append(255)  # stopval
        # print(ctime(), "TX:", encoded.hex(" "))
        self.reader.write(encoded)


if __name__ == "__main__":
    # usb = Usb('/dev/ttyACM0')

    # p = serialize_bytearray([10, 11, 12], 1, 2)
    # print(p)
    # print(p.as_bytearray().hex(" "))
    # print(parse_bytearray(p.as_bytearray()))
    exit()
