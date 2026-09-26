from setuptools import setup
from setuptools.dist import Distribution


class BinaryDistribution(Distribution):
    """Host-platform package.

    A wheel that contains the native trigora executable must not be tagged
    py3-none-any. Release wheels for other platforms stay out of this phase.
    """

    def has_ext_modules(self) -> bool:
        return True


setup(distclass=BinaryDistribution)
