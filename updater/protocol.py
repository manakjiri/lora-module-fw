from crc import Calculator, Crc32
from serial import Serial, SerialException, SerialTimeoutException
from serial.threaded import ReaderThread, Protocol
from queue import Queue, Empty
from time import ctime, time
from contextlib import contextmanager
from typing import Type

PREAMBLE = 0xF0
PADDING = 0x0F
TRANSFER_LENGTH = 16
PREAMBLE_LENGTH = 4
CHECKSUM_LENGTH = 4
MAX_ADDRESS = 254
MAX_PAYLOAD_SIZE = 1024


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

    def get_received(self, timeout=0.0) -> Packet | None:
        try:
            if not self.rx_queue.empty() or timeout:
                return self.rx_queue.get(timeout=timeout)
        except Empty:
            return None

    def get_all_received(self, timeout=0.0) -> list[Packet]:
        ret = []
        start = time()
        while not timeout and not self.rx_queue.empty():
            p = self.get_received(timeout)
            if p:
                ret.append(p)

            if (not p) or (timeout and start + timeout > time()):
                break

        return ret

    def transmit(self, p: Packet):
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

    # sends the current timestamp as string if data=None
    # expects to receive the same string within timeout seconds
    def ping_expect_echo(
        self, source_address, address, port, source_port=213, data=None, timeout=0.5
    ):
        if not data:
            data = str(round(time(), 3)).encode("ascii")

        # flush the receive buffer
        self.get_all_received(timeout=0.1)

        p1 = Packet(source_address, address, source_port, port, bytearray(data))
        self.transmit(p1)

        p2 = self.get_received(timeout=timeout)
        return bool(p2 and p2.payload() == p1.payload())


class UartReaderProtocol(Protocol):
    crc_calc = Calculator(Crc32.CRC32, optimized=True)
    rx_buffer = bytearray()
    last_index = 0
    callback = None

    def connection_made(self, transport):
        """Called when reader thread is started"""
        # print("Connected")

    def data_received(self, data):
        """Called with snippets received from the serial port"""
        # print(ctime(), "RX:", data.hex(" "))

        self.rx_buffer.extend(data)

        index = self.last_index
        while len(self.rx_buffer) - index >= TRANSFER_LENGTH and index < len(
            self.rx_buffer
        ):
            if self.rx_buffer[index] == PREAMBLE:
                try:
                    serialized = self.rx_buffer[index:]

                    assert len(serialized) >= TRANSFER_LENGTH * 2
                    for i in range(PREAMBLE_LENGTH):
                        assert serialized[i] == PREAMBLE

                    destination = serialized[PREAMBLE_LENGTH + 0]
                    source = serialized[PREAMBLE_LENGTH + 1]
                    payload_size = int.from_bytes(
                        serialized[PREAMBLE_LENGTH + 2 : PREAMBLE_LENGTH + 4],
                        "little",
                        signed=False,
                    )

                    assert (
                        source <= MAX_ADDRESS
                        and destination <= MAX_ADDRESS
                        and source
                        and destination
                        and source != destination
                    )
                    assert (
                        payload_size > 0
                        and payload_size <= MAX_PAYLOAD_SIZE
                        and payload_size
                        < len(serialized) - CHECKSUM_LENGTH - PREAMBLE_LENGTH
                    ), payload_size

                    payload = serialized[
                        CHECKSUM_LENGTH + 4 : CHECKSUM_LENGTH + 4 + payload_size
                    ]
                    assert len(payload) > 2

                    data_crc = self.crc_calc.checksum(
                        serialized[PREAMBLE_LENGTH : PREAMBLE_LENGTH + 4 + payload_size]
                    )
                    packet_crc = int.from_bytes(
                        serialized[
                            PREAMBLE_LENGTH
                            + 4
                            + payload_size : PREAMBLE_LENGTH
                            + 4
                            + payload_size
                            + CHECKSUM_LENGTH
                        ],
                        "little",
                        signed=False,
                    )
                    assert data_crc == packet_crc

                    index += len(serialized)

                    self.rx_buffer = self.rx_buffer[index:]
                    self.last_index = 0

                    if self.callback:
                        self.callback(
                            Packet(
                                source, destination, payload[1], payload[0], payload[2:]
                            )
                        )

                except Exception as e:
                    # print(e)
                    pass

            index += 1


class Uart(Interface):
    crc_calc = Calculator(Crc32.CRC32, optimized=True)

    def __init__(self, port, **kwargs) -> None:
        super().__init__(Serial(port, **kwargs), UartReaderProtocol)

    def transmit(self, p: Packet):
        assert len(p.payload()) > 0, '"payload" must contain at least one element'
        payload = bytearray([p.destination_port(), p.source_port()]) + p.payload()

        size = len(payload).to_bytes(2, "little")
        header = [p.destination_addr(), p.source_addr(), size[0], size[1]]
        body = bytearray(header) + bytearray(payload)
        checksum = self.crc_calc.checksum(body)

        serialized = bytearray([PREAMBLE] * PREAMBLE_LENGTH)
        serialized.extend(body)
        assert CHECKSUM_LENGTH == 4
        serialized.extend(checksum.to_bytes(CHECKSUM_LENGTH, "little"))

        if len(serialized) <= TRANSFER_LENGTH:
            serialized.extend(bytearray([0x0F] * TRANSFER_LENGTH))
        padding_len = TRANSFER_LENGTH - (len(serialized) % TRANSFER_LENGTH)
        if padding_len == TRANSFER_LENGTH:
            padding_len = 0

        serialized.extend(bytearray([PADDING] * padding_len))

        # print(ctime(), "TX:", serialized.hex(" "))
        self.reader.write(serialized)


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
                    data = maxval_decode(self.rx_buffer[:i], 250)
                    p = Packet(0, 0, data[1], data[0], data[2:])
                    self.callback(p)

                self.rx_buffer = self.rx_buffer[i + 1 :]
        except Exception as e:
            # print(e)
            pass


class Usb(Interface):
    def __init__(self, port, **kwargs) -> None:
        super().__init__(Serial(port, **kwargs), UsbReaderProtocol)

    def transmit(self, p: Packet):
        serialized = bytearray([p.destination_port(), p.source_port()])
        serialized.extend(p.payload())

        encoded = maxval_encode(serialized, 250)
        encoded.append(255)  # stopval
        # print(ctime(), "TX:", encoded.hex(" "))
        self.reader.write(encoded)


@contextmanager
def open_interface(
    port: str | None, interface_driver: Type[Interface] | None = None, **kwargs
):
    if interface_driver == None and port:
        port = str(port)
        if "USB" in port:
            interface = Uart(port, **kwargs)
        elif "ACM" in port:
            interface = Usb(port, **kwargs)
        else:
            raise NotImplementedError(f"unknown port type {port}")
    elif interface_driver and issubclass(interface_driver, Interface):
        interface = interface_driver(port, **kwargs)
    else:
        raise ValueError(f"invalid argument combination {port} {interface_driver}")

    try:
        yield interface
    finally:
        interface.close()


def find_device_on_port(
    ports: list[str],
    interface_driver: Type[Interface],
    device_address=1,
    device_echo_port=1,
    attempts=3,
) -> str | None:
    for port in ports:
        try:
            with open_interface(port, interface_driver) as interface:
                for _ in range(attempts):
                    if interface.ping_expect_echo(
                        100, device_address, device_echo_port
                    ):
                        return port

        except:
            pass

    return None


if __name__ == "__main__":
    # usb = Usb('/dev/ttyACM0')

    # p = serialize_packet([10, 11, 12], 1, 2)
    # print(p)
    # print(p.as_bytearray().hex(" "))
    # print(parse_packet(p.as_bytearray()))
    exit()
